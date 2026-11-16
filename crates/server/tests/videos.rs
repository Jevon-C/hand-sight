//! End-to-end tests of the video endpoints, driving the real router in memory with a fake
//! prober so no Python is needed.

use std::path::Path;
use std::sync::Arc;

use axonhand_api::{ApiError, ErrorCode, GateStatus, UploadResponse, Video, VideoList};
use axonhand_server::app::{build_jobs, router, Services};
use axonhand_server::probe::{ProbeFailure, ProbeResult, Prober};
use axonhand_server::store::Store;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BOUNDARY: &str = "XBOUNDARYX";
const LIMIT: u64 = 10_000;

fn mp4_bytes(extra: usize, seed: u8) -> Vec<u8> {
    let mut bytes = b"\0\0\0\x18ftypisom\0\0\0\0isomiso2".to_vec();
    bytes.extend(std::iter::repeat_n(seed, extra));
    bytes
}

fn good_prober() -> Prober {
    Arc::new(|_| {
        Ok(ProbeResult {
            width: 1920,
            height: 1080,
            fps: 60.0,
            duration_seconds: 12.0,
            frame_count: 720,
            codec: "h264".to_owned(),
        })
    })
}

struct Harness {
    app: Router,
    dir: tempfile::TempDir,
}

fn harness(prober: Prober) -> Result<Harness, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("videos"))?;
    std::fs::create_dir_all(dir.path().join("tmp"))?;
    std::fs::write(dir.path().join("index.html"), "<html></html>")?;
    let store = Arc::new(Store::open_in_memory()?);
    let registry_arc = Arc::new(axonhand_server::registry::Registry::default());
    let jobs = build_jobs(
        &store,
        &registry_arc,
        std::path::Path::new("python"),
        &dir.path().join("videos"),
        dir.path(),
    )?;
    let services = Services {
        store,
        prober,
        video_dir: dir.path().join("videos"),
        temp_dir: dir.path().join("tmp"),
        max_upload_bytes: LIMIT,
        registry: registry_arc,
        jobs,
    };
    Ok(Harness {
        app: router(dir.path(), services),
        dir,
    })
}

enum Part<'a> {
    Text(&'a str, &'a str),
    File(&'a str, &'a [u8]),
}

fn multipart(parts: &[Part<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts {
        body.extend(format!("--{BOUNDARY}\r\n").bytes());
        match part {
            Part::Text(name, value) => body.extend(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                    .bytes(),
            ),
            Part::File(filename, bytes) => {
                body.extend(
                    format!(
                        "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: video/mp4\r\n\r\n"
                    )
                    .bytes(),
                );
                body.extend(*bytes);
                body.extend(b"\r\n");
            }
        }
    }
    body.extend(format!("--{BOUNDARY}--\r\n").bytes());
    body
}

async fn send(
    app: &Router,
    request: Request<Body>,
) -> Result<(StatusCode, axum::http::HeaderMap, Vec<u8>), Box<dyn std::error::Error>> {
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await?.to_bytes().to_vec();
    Ok((status, headers, bytes))
}

async fn post(
    app: &Router,
    parts: &[Part<'_>],
) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let request = Request::builder()
        .method("POST")
        .uri("/api/videos")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(multipart(parts)))?;
    let (status, _, body) = send(app, request).await?;
    Ok((status, body))
}

async fn get(
    app: &Router,
    uri: &str,
) -> Result<(StatusCode, axum::http::HeaderMap, Vec<u8>), Box<dyn std::error::Error>> {
    send(app, Request::builder().uri(uri).body(Body::empty())?).await
}

fn good_form<'a>(file: &'a [u8]) -> Vec<Part<'a>> {
    vec![
        Part::Text("participant_code", "P-001"),
        Part::Text("visit_date", "2026-03-01"),
        Part::Text("hand", "left"),
        Part::File("clip.mp4", file),
    ]
}

fn error_of(body: &[u8]) -> Result<ApiError, serde_json::Error> {
    serde_json::from_slice(body)
}

fn leftovers(dir: &Path) -> Result<usize, std::io::Error> {
    Ok(std::fs::read_dir(dir.join("tmp"))?.count())
}

#[tokio::test]
async fn a_good_upload_is_stored_listed_and_described() -> TestResult {
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(100, 1);
    let (status, body) = post(&h.app, &good_form(&bytes)).await?;
    assert_eq!(status, StatusCode::CREATED);
    let uploaded: UploadResponse = serde_json::from_slice(&body)?;
    assert!(!uploaded.duplicate);
    let video = uploaded.video;
    assert_eq!(video.id.len(), 64);
    assert_eq!(video.participant_code, "P-001");
    assert_eq!(video.size_bytes, bytes.len() as u64);
    assert_eq!(video.filename, "clip.mp4");
    assert_eq!(video.gate[0].status, GateStatus::Pass);
    assert!(h
        .dir
        .path()
        .join("videos")
        .join(format!("{}.mp4", video.id))
        .is_file());
    assert_eq!(leftovers(h.dir.path())?, 0);

    let (_, _, body) = get(&h.app, "/api/videos").await?;
    let list: VideoList = serde_json::from_slice(&body)?;
    assert_eq!(list.videos, vec![video.clone()]);

    let (status, _, body) = get(&h.app, &format!("/api/videos/{}", video.id)).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<Video>(&body)?, video);
    Ok(())
}

#[tokio::test]
async fn the_id_is_the_sha256_of_the_bytes() -> TestResult {
    use sha2::{Digest, Sha256};
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(50, 7);
    let (_, body) = post(&h.app, &good_form(&bytes)).await?;
    let uploaded: UploadResponse = serde_json::from_slice(&body)?;
    let expected: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(uploaded.video.id, expected);
    Ok(())
}

#[tokio::test]
async fn the_same_file_twice_returns_the_first_record() -> TestResult {
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(100, 2);
    let (_, first) = post(&h.app, &good_form(&bytes)).await?;
    let mut again = good_form(&bytes);
    again[0] = Part::Text("participant_code", "OTHER");
    let (status, second) = post(&h.app, &again).await?;
    assert_eq!(status, StatusCode::OK);
    let first: UploadResponse = serde_json::from_slice(&first)?;
    let second: UploadResponse = serde_json::from_slice(&second)?;
    assert!(second.duplicate);
    assert_eq!(second.video, first.video);
    assert_eq!(std::fs::read_dir(h.dir.path().join("videos"))?.count(), 1);
    assert_eq!(leftovers(h.dir.path())?, 0);
    Ok(())
}

#[tokio::test]
async fn missing_or_bad_details_name_the_field_and_store_nothing() -> TestResult {
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(10, 3);
    let cases: Vec<(Vec<Part<'_>>, &str)> = vec![
        (
            vec![
                Part::Text("visit_date", "2026-03-01"),
                Part::Text("hand", "left"),
                Part::File("a.mp4", &bytes),
            ],
            "participant_code",
        ),
        (
            vec![
                Part::Text("participant_code", "P1"),
                Part::Text("hand", "left"),
                Part::File("a.mp4", &bytes),
            ],
            "visit_date",
        ),
        (
            vec![
                Part::Text("participant_code", "P1"),
                Part::Text("visit_date", "2026-02-30"),
                Part::Text("hand", "left"),
                Part::File("a.mp4", &bytes),
            ],
            "visit_date",
        ),
        (
            vec![
                Part::Text("participant_code", "P1"),
                Part::Text("visit_date", "2026-03-01"),
                Part::File("a.mp4", &bytes),
            ],
            "hand",
        ),
        (
            vec![
                Part::Text("participant_code", "P1"),
                Part::Text("visit_date", "2026-03-01"),
                Part::Text("hand", "middle"),
                Part::File("a.mp4", &bytes),
            ],
            "hand",
        ),
        (
            vec![
                Part::Text("participant_code", "../x"),
                Part::Text("visit_date", "2026-03-01"),
                Part::Text("hand", "left"),
                Part::File("a.mp4", &bytes),
            ],
            "participant_code",
        ),
        (
            vec![
                Part::Text("participant_code", "P1"),
                Part::Text("visit_date", "2026-03-01"),
                Part::Text("hand", "left"),
            ],
            "file",
        ),
    ];
    for (parts, field) in cases {
        let (status, body) = post(&h.app, &parts).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}");
        assert_eq!(error_of(&body)?.field.as_deref(), Some(field));
    }
    assert_eq!(std::fs::read_dir(h.dir.path().join("videos"))?.count(), 0);
    assert_eq!(leftovers(h.dir.path())?, 0);
    Ok(())
}

#[tokio::test]
async fn metadata_after_the_file_is_refused() -> TestResult {
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(10, 4);
    let parts = vec![
        Part::File("a.mp4", &bytes),
        Part::Text("participant_code", "P1"),
        Part::Text("visit_date", "2026-03-01"),
        Part::Text("hand", "left"),
    ];
    let (status, _) = post(&h.app, &parts).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn files_over_the_limit_are_refused_and_cleaned_up() -> TestResult {
    let h = harness(good_prober())?;
    let big = mp4_bytes(LIMIT as usize, 5);
    let (status, body) = post(&h.app, &good_form(&big)).await?;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error_of(&body)?.code, ErrorCode::TooLarge);
    assert_eq!(leftovers(h.dir.path())?, 0);
    Ok(())
}

#[tokio::test]
async fn content_decides_the_type_not_the_name() -> TestResult {
    let h = harness(good_prober())?;
    let (status, body) = post(
        &h.app,
        &good_form(b"MZ\x90\0 this is a program, not a video"),
    )
    .await?;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(error_of(&body)?.code, ErrorCode::UnsupportedMedia);
    let (status, _) = post(&h.app, &good_form(b"")).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(leftovers(h.dir.path())?, 0);
    Ok(())
}

#[tokio::test]
async fn a_video_the_probe_rejects_is_not_stored() -> TestResult {
    let prober: Prober = Arc::new(|_| {
        Err(ProbeFailure::NotAVideo(
            "the file is not a readable video".into(),
        ))
    });
    let h = harness(prober)?;
    let (status, body) = post(&h.app, &good_form(&mp4_bytes(20, 6))).await?;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(error_of(&body)?.message.contains("not a readable video"));
    assert_eq!(std::fs::read_dir(h.dir.path().join("videos"))?.count(), 0);
    assert_eq!(leftovers(h.dir.path())?, 0);
    Ok(())
}

#[tokio::test]
async fn a_broken_probe_is_a_server_error_without_details() -> TestResult {
    let prober: Prober = Arc::new(|_| {
        Err(ProbeFailure::Broken(
            "secret path /opt/python missing".into(),
        ))
    });
    let h = harness(prober)?;
    let (status, body) = post(&h.app, &good_form(&mp4_bytes(20, 8))).await?;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!String::from_utf8(body)?.contains("secret"));
    assert_eq!(leftovers(h.dir.path())?, 0);
    Ok(())
}

#[tokio::test]
async fn hostile_filenames_never_reach_the_file_system() -> TestResult {
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(30, 9);
    let parts = vec![
        Part::Text("participant_code", "P1"),
        Part::Text("visit_date", "2026-03-01"),
        Part::Text("hand", "right"),
        Part::File("../../../etc/passwd", &bytes),
    ];
    let (status, body) = post(&h.app, &parts).await?;
    assert_eq!(status, StatusCode::CREATED);
    let uploaded: UploadResponse = serde_json::from_slice(&body)?;
    assert_eq!(uploaded.video.filename, "passwd");
    let stored: Vec<_> =
        std::fs::read_dir(h.dir.path().join("videos"))?.collect::<Result<_, _>>()?;
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].file_name().to_string_lossy(),
        format!("{}.mp4", uploaded.video.id)
    );
    Ok(())
}

#[tokio::test]
async fn list_filters_by_participant_and_hand() -> TestResult {
    let h = harness(good_prober())?;
    for (code, hand, seed) in [("P1", "left", 10), ("P1", "right", 11), ("P2", "left", 12)] {
        let bytes = mp4_bytes(20, seed);
        let parts = vec![
            Part::Text("participant_code", code),
            Part::Text("visit_date", "2026-03-01"),
            Part::Text("hand", hand),
            Part::File("c.mp4", &bytes),
        ];
        assert_eq!(post(&h.app, &parts).await?.0, StatusCode::CREATED);
    }
    let count = |uri: &'static str| {
        let app = h.app.clone();
        async move {
            let (_, _, body) = get(&app, uri).await?;
            Ok::<_, Box<dyn std::error::Error>>(
                serde_json::from_slice::<VideoList>(&body)?.videos.len(),
            )
        }
    };
    assert_eq!(count("/api/videos").await?, 3);
    assert_eq!(count("/api/videos?participant_code=P1").await?, 2);
    assert_eq!(count("/api/videos?hand=left").await?, 2);
    assert_eq!(
        count("/api/videos?participant_code=P1&hand=right").await?,
        1
    );
    let (status, _, _) = get(&h.app, "/api/videos?hand=middle").await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn unknown_and_malformed_ids_are_not_found() -> TestResult {
    let h = harness(good_prober())?;
    for uri in [
        format!("/api/videos/{}", "a".repeat(64)),
        "/api/videos/not-an-id".to_owned(),
        "/api/videos/..%2F..%2Fetc%2Fpasswd".to_owned(),
        format!("/api/videos/{}/stream", "a".repeat(64)),
        "/api/videos/not-an-id/stream".to_owned(),
    ] {
        let (status, _, body) = get(&h.app, &uri).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(error_of(&body)?.code, ErrorCode::NotFound);
    }
    Ok(())
}

#[tokio::test]
async fn streaming_supports_byte_ranges() -> TestResult {
    let h = harness(good_prober())?;
    let bytes = mp4_bytes(1000, 13);
    let (_, body) = post(&h.app, &good_form(&bytes)).await?;
    let id = serde_json::from_slice::<UploadResponse>(&body)?.video.id;
    let uri = format!("/api/videos/{id}/stream");

    let (status, headers, body) = get(&h.app, &uri).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
    assert_eq!(
        headers.get(header::ACCEPT_RANGES).map(|v| v.as_bytes()),
        Some(&b"bytes"[..])
    );
    assert_eq!(
        headers.get(header::CONTENT_TYPE).map(|v| v.as_bytes()),
        Some(&b"video/mp4"[..])
    );

    let request = Request::builder()
        .uri(&uri)
        .header(header::RANGE, "bytes=10-19")
        .body(Body::empty())?;
    let (status, headers, body) = send(&h.app, request).await?;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, &bytes[10..20]);
    assert!(headers.get(header::CONTENT_RANGE).is_some());
    Ok(())
}
