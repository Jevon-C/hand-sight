//! Calls to the Hand Sight server from the browser.

use axonhand_api::{
    AnalysisResult, ClinicianScore, Job, JobRequest, LandmarkOverlay, ResultList, ScoreList,
    ScoreRequest, Signals, TestDetail, TestList, UploadMeta, UploadResponse, VideoList,
};
use gloo_net::http::Request;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{File, FormData, ProgressEvent, XmlHttpRequest};

use crate::format::{failure_message, hand_value, list_query};

/// Fetch the stored videos matching the filters, newest first.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_videos(
    participant_code: &str,
    hand: Option<axonhand_api::Hand>,
) -> Result<VideoList, String> {
    let url = format!("/api/videos{}", list_query(participant_code, hand));
    let response = Request::get(&url)
        .send()
        .await
        .map_err(|_| failure_message(0, ""))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !response.ok() {
        return Err(failure_message(status, &body));
    }
    serde_json::from_str(&body).map_err(|_| "The server sent an unreadable answer.".to_owned())
}

async fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|_| failure_message(0, ""))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !response.ok() {
        return Err(failure_message(status, &body));
    }
    serde_json::from_str(&body).map_err(|_| "The server sent an unreadable answer.".to_owned())
}

/// Fetch every installed test, including planned and broken ones.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_tests() -> Result<TestList, String> {
    get_json("/api/tests").await
}

/// Fetch one test in full.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_test(id: &str) -> Result<TestDetail, String> {
    get_json(&format!("/api/tests/{id}")).await
}

/// Ask the server to run a test on a video.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn start_job(request: &JobRequest) -> Result<Job, String> {
    let response = Request::post("/api/jobs")
        .json(request)
        .map_err(|_| "This browser could not prepare the request.".to_owned())?
        .send()
        .await
        .map_err(|_| failure_message(0, ""))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !response.ok() {
        return Err(failure_message(status, &body));
    }
    serde_json::from_str(&body).map_err(|_| "The server sent an unreadable answer.".to_owned())
}

/// Fetch a job's current state.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_job(id: &str) -> Result<Job, String> {
    get_json(&format!("/api/jobs/{id}")).await
}

/// Fetch the scores clinicians gave to a result.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_scores(key: &str) -> Result<ScoreList, String> {
    get_json(&format!("/api/results/{key}/scores")).await
}

/// Save one clinician's score for a result.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn save_score(key: &str, request: &ScoreRequest) -> Result<ClinicianScore, String> {
    let response = Request::put(&format!("/api/results/{key}/scores"))
        .json(request)
        .map_err(|_| "This browser could not prepare the request.".to_owned())?
        .send()
        .await
        .map_err(|_| failure_message(0, ""))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !response.ok() {
        return Err(failure_message(status, &body));
    }
    serde_json::from_str(&body).map_err(|_| "The server sent an unreadable answer.".to_owned())
}

/// Fetch the list of saved results, newest first.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_results() -> Result<ResultList, String> {
    get_json("/api/results").await
}

/// Fetch a saved result.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_result(key: &str) -> Result<AnalysisResult, String> {
    get_json(&format!("/api/results/{key}")).await
}

/// Fetch the data behind a result's plots.
///
/// # Errors
///
/// A sentence fit to show the user.
pub async fn fetch_signals(key: &str) -> Result<Signals, String> {
    get_json(&format!("/api/results/{key}/signals")).await
}

/// Fetch the hand landmarks of every frame of a result's video, for drawing over it.
///
/// # Errors
///
/// A sentence fit to show the user (a result made before landmarks were kept has none).
pub async fn fetch_landmarks(key: &str) -> Result<LandmarkOverlay, String> {
    get_json(&format!("/api/results/{key}/landmarks")).await
}

/// Send one video. `on_progress` receives a fraction between 0 and 1 while bytes are sent;
/// `on_done` is called once, with the stored video or a sentence to show the user.
///
/// The three event callbacks are leaked on purpose: they must outlive this function, the
/// browser owns the request, and each upload leaks only a few small closures.
pub fn upload_video(
    meta: &UploadMeta,
    file: &File,
    on_progress: impl Fn(f64) + 'static,
    on_done: impl Fn(Result<UploadResponse, String>) + 'static,
) {
    let on_done = std::rc::Rc::new(on_done);
    let Some(request) = start() else {
        on_done(Err("This browser could not start the upload.".to_owned()));
        return;
    };
    let progress = Closure::<dyn Fn(ProgressEvent)>::new(move |event: ProgressEvent| {
        if event.length_computable() && event.total() > 0.0 {
            on_progress(event.loaded() / event.total());
        }
    });
    if let Ok(upload) = request.upload() {
        upload.set_onprogress(Some(progress.as_ref().unchecked_ref()));
    }
    progress.forget();

    let finished = request.clone();
    let done = on_done.clone();
    let load = Closure::<dyn Fn()>::new(move || {
        let status = finished.status().unwrap_or(0);
        let body = finished.response_text().ok().flatten().unwrap_or_default();
        let result = if (200..300).contains(&status) {
            serde_json::from_str(&body)
                .map_err(|_| "The server sent an unreadable answer.".to_owned())
        } else {
            Err(failure_message(status, &body))
        };
        done(result);
    });
    request.set_onload(Some(load.as_ref().unchecked_ref()));
    load.forget();

    let failed = on_done.clone();
    let error = Closure::<dyn Fn()>::new(move || failed(Err(failure_message(0, ""))));
    request.set_onerror(Some(error.as_ref().unchecked_ref()));
    error.forget();

    if let Err(error) = send_form(&request, meta, file) {
        web_sys::console::error_1(&error);
        on_done(Err("This browser could not send the video.".to_owned()));
    }
}

fn start() -> Option<XmlHttpRequest> {
    let request = XmlHttpRequest::new().ok()?;
    request.open_with_async("POST", "/api/videos", true).ok()?;
    Some(request)
}

/// Details go before the file so the server can refuse a bad form before reading the video.
fn send_form(
    request: &XmlHttpRequest,
    meta: &UploadMeta,
    file: &File,
) -> Result<(), wasm_bindgen::JsValue> {
    let form = FormData::new()?;
    form.append_with_str("participant_code", &meta.participant_code)?;
    form.append_with_str("visit_date", &meta.visit_date)?;
    form.append_with_str("hand", hand_value(meta.hand))?;
    form.append_with_blob_and_filename("file", file, &file.name())?;
    request.send_with_opt_form_data(Some(&form))
}
