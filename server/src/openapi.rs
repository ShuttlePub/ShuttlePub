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
        schema::{AdditionalProperties, OneOfBuilder, SchemaType, Type},
        security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    },
};
use utoipa_axum::router::OpenApiRouter;

use crate::api_v1::{self, personal_domain, quick_reaction, search_author};

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
        quick_reaction::EmojiImageState,
        quick_reaction::EmojiRef,
        quick_reaction::EmojiUnavailableReason,
        quick_reaction::EmojiUsage,
        quick_reaction::EmojiView,
        quick_reaction::ProfileNanoid,
        quick_reaction::PutReaction,
        quick_reaction::QuickReactionSettings,
        quick_reaction::ReactionState,
        quick_reaction::RevisionConflictBody,
        quick_reaction::UpdateQuickReactionSettings,
        personal_domain::DomainControlRef,
        personal_domain::NoteSearchItem,
        personal_domain::SearchNoteBody,
        personal_domain::SearchedNote,
        personal_domain::ActorUrl,
        personal_domain::InboundReplyRejection,
        personal_domain::InboundQuoteRejection,
        personal_domain::InboundRepostRejection,
        personal_domain::InboundLikeRejection,
        personal_domain::InboundEmojiReactionRejection,
        search_author::ResolveAuthorResponse,
        search_author::SearchAuthor,
        search_author::SuggestAuthorsResponse,
    )),
    tags(
        (name = "meta", description = "Service level information"),
        (name = "quick-reaction", description = "Quick reaction favourites per Profile and the reaction of a Profile on a Note. Contract only: stored nowhere, every route answers 501."),
        (name = "personal-domain-content", description = "S8 personal domain display and post refusal contracts. E8 owns policy; no filtering or enforcement is implemented. All routes answer 501. Inbound DTOs are internal decision contracts, not an inbox."),
        (name = "note-search-author", description = "D04 author lookup for the search UI's from:@acct input: resolve one acct, suggest up to five authors. Contract only: every route answers 501. The server never queries a remote server (no WebFinger, no actor fetch).")
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
            // utoipa does not close tagged enum variants from deny_unknown_fields.
            // Close every S8 sum type too: hidden payloads and feed items must
            // reject arbitrary body/author/media/placeholder extensions.
            for name in [
                "EmojiRef",
                "FeedItem",
                "NoteSearchItem",
                "SearchNoteBody",
                "EmbeddedNotePresentation",
                "DirectItem",
                "DomainOperationPermission",
                "NotePresentation",
            ] {
                if let Some(RefOr::T(Schema::OneOf(one_of))) = components.schemas.get_mut(name) {
                    for variant in &mut one_of.items {
                        if let RefOr::T(Schema::Object(object)) = variant {
                            object.additional_properties =
                                Some(Box::new(AdditionalProperties::FreeForm(false)));
                        }
                    }
                }
            }
            // utoipa has no container-level maxLength for a newtype.
            if let Some(RefOr::T(Schema::Object(actor_url))) =
                components.schemas.get_mut("ActorUrl")
            {
                actor_url.max_length = Some(personal_domain::NOTE_SEARCH_MAX_AUTHOR_CHARS);
            }
            // A truncated search page must resume: express "truncated: true
            // requires next_cursor" as a oneOf on the `truncated` literal, which
            // a plain struct schema cannot say. Both variants stay closed.
            if let Some(RefOr::T(Schema::Object(page))) =
                components.schemas.get("NoteSearchPageInfo").cloned()
            {
                let variant = |truncated: bool| {
                    let mut object = page.clone();
                    object.description = None;
                    if let Some(RefOr::T(Schema::Object(flag))) =
                        object.properties.get_mut("truncated")
                    {
                        flag.enum_values = Some(vec![Value::Bool(truncated)]);
                    }
                    if let Some(RefOr::T(Schema::Object(cursor))) =
                        object.properties.get_mut("next_cursor")
                    {
                        // Option<String> is generated nullable; null is not a cursor.
                        cursor.schema_type = SchemaType::Type(Type::String);
                    }
                    if truncated && !object.required.iter().any(|r| r == "next_cursor") {
                        object.required.push("next_cursor".to_owned());
                    }
                    object.additional_properties =
                        Some(Box::new(AdditionalProperties::FreeForm(false)));
                    RefOr::T(Schema::Object(object))
                };
                let mut one_of = OneOfBuilder::new()
                    .item(variant(false))
                    .item(variant(true))
                    .build();
                one_of.description = page.description.clone();
                components.schemas.insert(
                    "NoteSearchPageInfo".to_owned(),
                    RefOr::T(Schema::OneOf(one_of)),
                );
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
    let (router, mut doc) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(api_v1::router())
        .split_for_parts();
    // routes! contributes referenced schemas during the merge. Apply fixups
    // afterwards so newly route-discovered enum variants are closed as well.
    ContractFixups.modify(&mut doc);
    (router, doc)
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
