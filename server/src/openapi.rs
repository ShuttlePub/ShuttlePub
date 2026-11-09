//! OpenAPI document for the `/api/v1` contract.
//!
//! The document is generated from the Rust types with utoipa and rendered
//! deterministically so that `openapi/shuttlepub.json` can be checked in and
//! diffed in CI.

use serde_json::{Map, Value};
use utoipa::OpenApi;

use crate::api_v1;

/// Checked-in location of the generated document, relative to the workspace root.
pub const GENERATED_PATH: &str = "openapi/shuttlepub.json";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "ShuttlePub API",
        description = "ShuttlePub /api/v1 contract. Routes and types only: handlers are stubs answering 501 until the backend is implemented."
    ),
    paths(api_v1::get_status),
    components(schemas(
        api_v1::ApiStatus,
        api_v1::ErrorBody,
        api_v1::ErrorCode,
        api_v1::FieldViolation,
        api_v1::PageInfo,
        api_v1::PageParams,
    )),
    tags((name = "meta", description = "Service level information"))
)]
pub struct ApiDoc;

/// Recursively sort object keys so the output never depends on map ordering.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// Render the document: keys sorted, two-space indentation, trailing newline.
pub fn render() -> Result<String, serde_json::Error> {
    let value = sorted(serde_json::to_value(ApiDoc::openapi())?);
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}
