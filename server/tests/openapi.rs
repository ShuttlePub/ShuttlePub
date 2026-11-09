use application::NoteService;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn app() -> Result<Router, Box<dyn std::error::Error>> {
    let notes = Arc::new(NoteService::in_memory().await);
    let key = Arc::new(driver::DevelopmentKey::generate()?);
    let federation = Arc::new(driver::Federation::new(
        "https://local.example/actor",
        vec![],
        key,
    )?);
    Ok(server::router(notes, federation))
}

fn generated_file() -> Result<String, std::io::Error> {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(server::openapi::GENERATED_PATH),
    )
}

async fn body_of(
    response: axum::response::Response,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(response.into_body().collect().await?.to_bytes().to_vec())
}

#[test]
fn checked_in_schema_matches_generated_output() -> TestResult {
    let generated = server::openapi::render()?;
    assert_eq!(
        generated_file()?,
        generated,
        "openapi/shuttlepub.json is stale; run `cargo run --locked -p shuttlepub-server --bin openapi`"
    );
    Ok(())
}

#[test]
fn rendering_is_deterministic() -> TestResult {
    let first = server::openapi::render()?;
    assert_eq!(first, server::openapi::render()?);
    assert!(first.ends_with("}\n"));
    let value: Value = serde_json::from_str(&first)?;
    assert_eq!(value["openapi"], "3.1.0");
    assert_eq!(serde_json::to_string_pretty(&value)? + "\n", first);
    Ok(())
}

#[tokio::test]
async fn served_schema_is_byte_identical_to_checked_in_file() -> TestResult {
    let response = app()
        .await?
        .oneshot(Request::get("/openapi.json").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(body_of(response).await?, generated_file()?.into_bytes());
    Ok(())
}

fn operations(spec: &Value) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for (path, item) in spec["paths"].as_object().into_iter().flatten() {
        for (method, operation) in item.as_object().into_iter().flatten() {
            let id = operation["operationId"].as_str().unwrap_or_default();
            found.push((method.to_uppercase(), path.clone(), id.to_owned()));
        }
    }
    found
}

#[test]
fn route_and_schema_snapshot() -> TestResult {
    let spec: Value = serde_json::from_str(&server::openapi::render()?)?;
    assert_eq!(
        operations(&spec),
        vec![("GET".into(), "/api/v1/status".into(), "getApiStatus".into())]
    );
    let schemas: Vec<&str> = spec["components"]["schemas"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        schemas,
        [
            "ApiStatus",
            "ErrorBody",
            "ErrorCode",
            "FieldViolation",
            "PageInfo",
            "PageParams"
        ]
    );
    assert_eq!(
        spec["components"]["schemas"]["ErrorCode"]["enum"],
        serde_json::json!([
            "invalid_request",
            "unauthorized",
            "forbidden",
            "not_found",
            "conflict",
            "internal",
            "not_implemented"
        ])
    );
    assert_eq!(
        spec["components"]["schemas"]["ErrorBody"]["required"],
        serde_json::json!(["code", "message"])
    );
    Ok(())
}

#[test]
fn schema_excludes_legacy_development_api() -> TestResult {
    let spec: Value = serde_json::from_str(&server::openapi::render()?)?;
    for path in spec["paths"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(p, _)| p)
    {
        assert!(path.starts_with("/api/v1/"), "unexpected path {path}");
    }
    Ok(())
}

#[tokio::test]
async fn every_documented_route_is_an_explicit_501_stub() -> TestResult {
    let spec: Value = serde_json::from_str(&server::openapi::render()?)?;
    let app = app().await?;
    let documented = operations(&spec);
    assert!(!documented.is_empty());
    for (method, path, _) in documented {
        let uri = path.replace(['{', '}'], "x");
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.as_str())
                    .uri(&uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::NOT_IMPLEMENTED,
            "{method} {path}"
        );
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let body: Value = serde_json::from_slice(&body_of(response).await?)?;
        assert_eq!(body["code"], "not_implemented", "{method} {path}");
        assert!(body["message"].is_string());
    }
    Ok(())
}
