//! OpenAPI document for the `/api/v1` contract.
//!
//! The document is generated from the Rust types with utoipa and rendered
//! deterministically so that `openapi/shuttlepub.json` can be checked in and
//! diffed in CI.

use axum::Router;
use serde_json::{Map, Value};
use utoipa::{
    Modify, OpenApi,
    openapi::{
        RefOr, Schema,
        schema::AdditionalProperties,
        security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    },
};
use utoipa_axum::router::OpenApiRouter;

use crate::api_v1::{self, quick_reaction};

/// Checked-in location of the generated document, relative to the workspace root.
pub const GENERATED_PATH: &str = "openapi/shuttlepub.json";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "ShuttlePub API",
        description = "ShuttlePub /api/v1 contract. Routes and types only: handlers are stubs answering 501 until the backend is implemented."
    ),
    components(schemas(
        api_v1::ApiStatus,
        api_v1::ErrorBody,
        api_v1::ErrorCode,
        api_v1::FieldViolation,
        api_v1::PageInfo,
        api_v1::PageParams,
        quick_reaction::EmojiAvailability,
        quick_reaction::EmojiRef,
        quick_reaction::EmojiUnavailableReason,
        quick_reaction::EmojiView,
        quick_reaction::ProfileNanoid,
        quick_reaction::PutReaction,
        quick_reaction::QuickReactionSettings,
        quick_reaction::ReactionState,
        quick_reaction::RevisionConflictBody,
        quick_reaction::UpdateQuickReactionSettings,
    )),
    modifiers(&ContractFixups),
    tags(
        (name = "meta", description = "Service level information"),
        (name = "quick-reaction", description = "Quick reaction favourites per Profile and the reaction of a Profile on a Note. Contract only: stored nowhere, every route answers 501.")
    )
)]
pub struct ApiDoc;

/// Registers the `bearer_auth` scheme referenced by authenticated operations
/// and closes the `EmojiRef` variants: utoipa does not emit
/// `additionalProperties: false` for variants, yet an emoji identity is the
/// whole JSON value (this is what makes `uniqueItems` mean "distinct emoji").
struct ContractFixups;

impl Modify for ContractFixups {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            if let Some(RefOr::T(Schema::OneOf(one_of))) = components.schemas.get_mut("EmojiRef") {
                for variant in &mut one_of.items {
                    if let RefOr::T(Schema::Object(object)) = variant {
                        object.additional_properties =
                            Some(Box::new(AdditionalProperties::FreeForm(false)));
                    }
                }
            }
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some(
                            "Bearer access token of the signed-in user. The caller must be allowed to act as the Profile in the path.",
                        ))
                        .build(),
                ),
            );
        }
    }
}

/// Axum router and OpenAPI document built from the single route registration in
/// [`api_v1::router`].
pub fn contract() -> (Router, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(api_v1::router())
        .split_for_parts()
}

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
    let value = sorted(serde_json::to_value(contract().1)?);
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}
