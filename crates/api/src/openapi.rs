//! OpenAPI description of the API, generated from the shared types.

use utoipa::openapi::path::{HttpMethod, OperationBuilder, PathItem};
use utoipa::openapi::{
    ContentBuilder, InfoBuilder, OpenApi, OpenApiBuilder, PathsBuilder, Ref, RefOr, ResponseBuilder,
};
use utoipa::{PartialSchema, ToSchema};

use crate::{
    AnalysisResult, ApiError, CaptureRules, ClinicianScore, ErrorCode, EventData, GateCheck,
    GateCheckName, GateStatus, Hand, Health, HealthStatus, Job, JobRequest, JobState,
    LandmarkOverlay, MeasureValue, MetricDef, ModelInfo, ParameterDef, ParameterKind, QualityCheck,
    QualityStatus, ResultList, ResultSummary, ScoreList, ScoreRequest, SeriesData, Signals,
    TestDetail, TestList, TestStatus, TestSummary, UploadMeta, UploadResponse, Video, VideoList,
};

fn json_response(description: &str, schema_name: &str) -> RefOr<utoipa::openapi::Response> {
    RefOr::T(
        ResponseBuilder::new()
            .description(description)
            .content(
                "application/json",
                ContentBuilder::new()
                    .schema(Some(Ref::from_schema_name(schema_name)))
                    .build(),
            )
            .build(),
    )
}

fn document() -> OpenApi {
    let health = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("health"))
            .summary(Some("Report that the server is running."))
            .response("200", json_response("The server is ready.", "Health"))
            .response("500", json_response("Unexpected server error.", "ApiError")),
    );
    let upload = OperationBuilder::new()
        .operation_id(Some("upload_video"))
        .summary(Some(
            "Store one video. Multipart form with `participant_code`, `visit_date`, `hand` and `file`.",
        ))
        .response("201", json_response("Stored.", "UploadResponse"))
        .response("200", json_response("An identical file was already stored.", "UploadResponse"))
        .response("400", json_response("A field is missing or invalid.", "ApiError"))
        .response("413", json_response("The file is too large.", "ApiError"))
        .response("415", json_response("The file is not a readable video.", "ApiError"));
    let list = OperationBuilder::new()
        .operation_id(Some("list_videos"))
        .summary(Some(
            "List stored videos, newest first. Optional filters: `participant_code`, `hand`.",
        ))
        .response("200", json_response("The matching videos.", "VideoList"));
    let mut videos = PathItem::new(HttpMethod::Post, upload);
    videos.merge_operations(PathItem::new(HttpMethod::Get, list));
    let one = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("get_video"))
            .summary(Some("Describe one stored video."))
            .response("200", json_response("The video.", "Video"))
            .response("404", json_response("No such video.", "ApiError")),
    );
    let tests = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("list_tests"))
            .summary(Some(
                "List installed tests, including planned and broken ones.",
            ))
            .response("200", json_response("The tests.", "TestList")),
    );
    let one_test = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("get_test"))
            .summary(Some(
                "Describe one test: capture rules, metrics and settings.",
            ))
            .response("200", json_response("The test.", "TestDetail"))
            .response("404", json_response("No such test.", "ApiError")),
    );
    let create_job = PathItem::new(
        HttpMethod::Post,
        OperationBuilder::new()
            .operation_id(Some("create_job"))
            .summary(Some(
                "Run a test on a video. JSON body `JobRequest`. Asking again for the same \
                 video, test version, model and settings returns the existing job.",
            ))
            .response("202", json_response("Work was queued.", "Job"))
            .response(
                "200",
                json_response(
                    "The result already exists or the same work is already waiting.",
                    "Job",
                ),
            )
            .response(
                "400",
                json_response(
                    "The request is invalid; the error names the field.",
                    "ApiError",
                ),
            )
            .response("404", json_response("No such video.", "ApiError")),
    );
    let get_job = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("get_job"))
            .summary(Some("Report a job's state and progress."))
            .response("200", json_response("The job.", "Job"))
            .response("404", json_response("No such job.", "ApiError")),
    );
    let job_events = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("job_events"))
            .summary(Some(
                "Server-sent events (`event: job`, data is a `Job`): the current state at once, \
                 then every change, ending when the job succeeds or fails.",
            ))
            .response(
                "200",
                RefOr::T(
                    ResponseBuilder::new()
                        .description("An event stream.")
                        .build(),
                ),
            )
            .response("404", json_response("No such job.", "ApiError")),
    );
    let list_results = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("list_results"))
            .summary(Some(
                "List saved results, newest first. Optional filters: `test_id`, `participant_code`.",
            ))
            .response("200", json_response("The matching results.", "ResultList")),
    );
    let get_result = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("get_result"))
            .summary(Some("Read a saved result."))
            .response("200", json_response("The result.", "AnalysisResult"))
            .response("404", json_response("No such result.", "ApiError")),
    );
    let get_signals = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("get_signals"))
            .summary(Some("Read the data behind a result's plots."))
            .response("200", json_response("The signals.", "Signals"))
            .response("404", json_response("No such result.", "ApiError")),
    );
    let get_landmarks = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("get_landmarks"))
            .summary(Some(
                "Read the hand landmarks of every frame, for drawing over the video.",
            ))
            .response("200", json_response("The landmarks.", "LandmarkOverlay"))
            .response(
                "404",
                json_response("No such result, or it has no landmarks.", "ApiError"),
            ),
    );
    let list_scores = OperationBuilder::new()
        .operation_id(Some("list_scores"))
        .summary(Some("List the scores clinicians gave to a result."))
        .response("200", json_response("The scores.", "ScoreList"))
        .response("404", json_response("No such result.", "ApiError"));
    let put_score = OperationBuilder::new()
        .operation_id(Some("put_score"))
        .summary(Some(
            "Save one clinician's score (0 to 4) for a result. JSON body `ScoreRequest`. \
             Saving again with the same rater replaces that rater's score.",
        ))
        .response("200", json_response("The saved score.", "ClinicianScore"))
        .response("400", json_response("A field is invalid.", "ApiError"))
        .response("404", json_response("No such result.", "ApiError"));
    let mut scores = PathItem::new(HttpMethod::Get, list_scores);
    scores.merge_operations(PathItem::new(HttpMethod::Put, put_score));
    let export_csv = PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .operation_id(Some("export_result_csv"))
            .summary(Some(
                "Download a result as CSV, with header lines naming the test, version, model and settings.",
            ))
            .response(
                "200",
                RefOr::T(ResponseBuilder::new().description("The CSV file.").build()),
            )
            .response("404", json_response("No such result.", "ApiError")),
    );
    OpenApiBuilder::new()
        .info(
            InfoBuilder::new()
                .title("Hand Sight API")
                .version(env!("CARGO_PKG_VERSION"))
                .description(Some(
                    "Research prototype. Results are research-grade measures, not clinical scores.",
                ))
                .build(),
        )
        .paths(
            PathsBuilder::new()
                .path("/api/health", health)
                .path("/api/videos", videos)
                .path("/api/videos/{id}", one)
                .path("/api/tests", tests)
                .path("/api/tests/{id}", one_test)
                .path("/api/jobs", create_job)
                .path("/api/jobs/{id}", get_job)
                .path("/api/jobs/{id}/events", job_events)
                .path("/api/results", list_results)
                .path("/api/results/{key}", get_result)
                .path("/api/results/{key}/signals", get_signals)
                .path("/api/results/{key}/landmarks", get_landmarks)
                .path("/api/results/{key}/scores", scores)
                .path("/api/results/{key}/export.csv", export_csv)
                .build(),
        )
        .components(Some(
            utoipa::openapi::ComponentsBuilder::new()
                .schema(Health::name(), Health::schema())
                .schema(HealthStatus::name(), HealthStatus::schema())
                .schema(ApiError::name(), ApiError::schema())
                .schema(ErrorCode::name(), ErrorCode::schema())
                .schema(Hand::name(), Hand::schema())
                .schema(GateStatus::name(), GateStatus::schema())
                .schema(GateCheckName::name(), GateCheckName::schema())
                .schema(GateCheck::name(), GateCheck::schema())
                .schema(UploadMeta::name(), UploadMeta::schema())
                .schema(Video::name(), Video::schema())
                .schema(VideoList::name(), VideoList::schema())
                .schema(UploadResponse::name(), UploadResponse::schema())
                .schema(TestStatus::name(), TestStatus::schema())
                .schema(CaptureRules::name(), CaptureRules::schema())
                .schema(MetricDef::name(), MetricDef::schema())
                .schema(ParameterKind::name(), ParameterKind::schema())
                .schema(ParameterDef::name(), ParameterDef::schema())
                .schema(TestSummary::name(), TestSummary::schema())
                .schema(TestDetail::name(), TestDetail::schema())
                .schema(TestList::name(), TestList::schema())
                .schema(JobState::name(), JobState::schema())
                .schema(JobRequest::name(), JobRequest::schema())
                .schema(Job::name(), Job::schema())
                .schema(ModelInfo::name(), ModelInfo::schema())
                .schema(MeasureValue::name(), MeasureValue::schema())
                .schema(QualityStatus::name(), QualityStatus::schema())
                .schema(QualityCheck::name(), QualityCheck::schema())
                .schema(AnalysisResult::name(), AnalysisResult::schema())
                .schema(SeriesData::name(), SeriesData::schema())
                .schema(EventData::name(), EventData::schema())
                .schema(Signals::name(), Signals::schema())
                .schema(LandmarkOverlay::name(), LandmarkOverlay::schema())
                .schema(ResultSummary::name(), ResultSummary::schema())
                .schema(ResultList::name(), ResultList::schema())
                .schema(ScoreRequest::name(), ScoreRequest::schema())
                .schema(ClinicianScore::name(), ClinicianScore::schema())
                .schema(ScoreList::name(), ScoreList::schema())
                .build(),
        ))
        .build()
}

/// The API description as pretty-printed JSON, ending in a newline.
///
/// # Errors
///
/// Returns the serialisation error if the document cannot be rendered, which would be a bug.
pub fn openapi_json() -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(&document())?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_health_endpoint_and_its_error_shape() -> Result<(), serde_json::Error> {
        let text = openapi_json()?;
        assert!(text.contains("\"/api/health\""));
        assert!(text.contains("\"ApiError\""));
        assert!(text.contains("\"invalid_request\""));
        Ok(())
    }

    /// The checked-in file must match the generator. To refresh it, run
    /// `cargo run -p axonhand-api --example write_openapi`.
    #[test]
    fn checked_in_file_is_current() -> Result<(), Box<dyn std::error::Error>> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/api/openapi.json");
        let on_disk = std::fs::read_to_string(path)?;
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            openapi_json()?,
            "docs/api/openapi.json is out of date"
        );
        Ok(())
    }
}
