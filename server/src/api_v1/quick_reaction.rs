//! Quick reaction contract (D03 / execution unit `quick-reaction-api-contract`).
//!
//! Two resources hang off a posting Profile:
//!
//! * `/api/v1/profiles/{profile_nanoid}/quick-reaction-settings` — the ordered
//!   favourite emoji (0..=5) shown at the top of the reaction picker, shared by
//!   every device of the Profile and guarded by an optimistic `revision`.
//! * `/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction` — the single
//!   reaction a Profile has on a Note (1 Profile, 1 Note, 1 reaction).
//!
//! **Contract only.** Nothing is stored and nothing is synchronised: every
//! handler is an explicit `501 Not Implemented` stub. Types, validation helpers
//! and documented status codes are what clients (the web UI and its stateful
//! mock) code against. The legacy development route
//! `POST /api/notes/{id}/reactions` is not part of this contract.
//!
//! # Identifier choices
//!
//! * `profile_nanoid` is the public Profile identifier of Emumet (its
//!   `/api/v1/profiles/{profile_nanoid}` routes use the same value). Emumet's
//!   `Nanoid` is generated with the default nanoid alphabet (`A-Za-z0-9_-`),
//!   which is what [`ProfileNanoid`] accepts (1..=64 characters).
//! * `note_id` is the ShuttlePub `NoteId`, a UUID (the same value the legacy
//!   development API takes as `{id}`).
//!
//! # Semantics
//!
//! * Settings `PUT` is a full replacement guarded by `expected_revision`.
//!   A never-saved Profile reads as revision `0` with no favourites. A `PUT`
//!   whose `favorites` already equal the stored list answers `200` with the
//!   stored settings and does **not** bump the revision, whatever
//!   `expected_revision` says; that makes a retry after a lost response
//!   succeed with the same result. Any other mismatch answers
//!   `409 revision_conflict` carrying `current_revision`.
//! * Favourites that became unavailable stay in the list (no automatic
//!   deletion, no substitution). A settings `PUT` may keep them, so reordering
//!   works, but may not newly add an unavailable emoji (`422 emoji_unavailable`).
//! * Reaction `PUT` selects the emoji (`200`). Repeating the same emoji is a
//!   no-op `200`; a different emoji replaces the previous one. The picker UI
//!   removes first and then selects, but the replacement is part of the API.
//!   Selecting an unavailable emoji answers `422 emoji_unavailable`.
//! * Reaction `DELETE` always answers `200` with the unreacted state, also when
//!   nothing was selected (retry safe) and **also when the selected emoji has
//!   become unavailable**: removal never depends on availability.
//! * Reaction updates carry no revision: the resource holds one value, so
//!   concurrent requests resolve last-write-wins.
//! * Validation errors (`invalid_request`) are reported with per-field
//!   [`FieldViolation`]s whose `field` is a JSON pointer into the body.
//!
//! Order of checks (when implemented): authentication (401), Profile
//! permission (403), existence (404), input validation (400), then the
//! state-dependent outcomes above. Stubs skip all of them and answer 501.

use serde::{Deserialize, Serialize};
use utoipa::{
    ToSchema,
    openapi::{ArrayBuilder, ObjectBuilder, Ref, RefOr, Schema, Type},
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use super::{ApiV1Error, ErrorBody, ErrorCode, FieldViolation};

/// Maximum number of favourite emoji per Profile (decision d03-q02).
pub const MAX_FAVORITES: usize = 5;
/// Maximum length of a Unicode emoji value, in code points.
pub const MAX_UNICODE_EMOJI_CODE_POINTS: usize = 32;
/// Maximum length of a custom emoji identifier, in characters.
pub const MAX_CUSTOM_EMOJI_ID_LEN: usize = 128;
/// Maximum length of a Profile nanoid, in characters.
pub const MAX_PROFILE_NANOID_LEN: usize = 64;

fn violation(field: impl Into<String>, message: impl Into<String>) -> FieldViolation {
    FieldViolation {
        field: field.into(),
        message: message.into(),
    }
}

/// Public identifier of a posting Profile (Emumet's `profile_nanoid`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(pattern = r"^[A-Za-z0-9_-]{1,64}$", example = "V1StGXR8_Z5jdHi6B-myT")]
pub struct ProfileNanoid(pub String);

impl ProfileNanoid {
    /// Check the identifier format; `field` names the offending location.
    pub fn validate(&self, field: &str) -> Result<(), FieldViolation> {
        let len = self.0.chars().count();
        let valid_chars = self
            .0
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if len == 0 || len > MAX_PROFILE_NANOID_LEN || !valid_chars {
            return Err(violation(
                field,
                format!("must be 1..={MAX_PROFILE_NANOID_LEN} characters of A-Z a-z 0-9 _ -"),
            ));
        }
        Ok(())
    }
}

/// Identity of an emoji. Two references are the same emoji when they are equal
/// as JSON values, which is why this type carries no display data.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmojiRef {
    /// A Unicode emoji, identified by its code point sequence.
    Unicode {
        /// The fully qualified emoji sequence, for example `👍` or `👨‍👩‍👧‍👦`.
        /// At most 32 code points, no whitespace or control characters and at
        /// least one non-ASCII code point (the exact emoji set is checked by
        /// the server; skin tones and ZWJ sequences are distinct emoji).
        #[schema(
            min_length = 1,
            max_length = 32,
            pattern = r"[^\x00-\x7F]",
            example = "👍"
        )]
        value: String,
    },
    /// A custom emoji, identified by the server's stable emoji identifier.
    /// The identifier survives deletion, so an unavailable emoji keeps its
    /// identity.
    Custom {
        /// Stable identifier of the custom emoji (not the shortcode).
        #[schema(
            min_length = 1,
            max_length = 128,
            pattern = r"^[A-Za-z0-9_.:@-]+$",
            example = "blobcat"
        )]
        id: String,
    },
}

impl EmojiRef {
    /// Validate the identifier; `field` is the JSON pointer of this value.
    pub fn validate(&self, field: &str) -> Result<(), FieldViolation> {
        match self {
            Self::Unicode { value } => {
                let count = value.chars().count();
                let blocked = value.chars().any(|c| c.is_whitespace() || c.is_control());
                if count == 0
                    || count > MAX_UNICODE_EMOJI_CODE_POINTS
                    || blocked
                    || value.is_ascii()
                {
                    return Err(violation(
                        format!("{field}/value"),
                        format!(
                            "must be an emoji sequence of 1..={MAX_UNICODE_EMOJI_CODE_POINTS} \
                             code points without whitespace or control characters"
                        ),
                    ));
                }
            }
            Self::Custom { id } => {
                let valid_chars = id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '@' | '-'));
                if id.is_empty() || id.chars().count() > MAX_CUSTOM_EMOJI_ID_LEN || !valid_chars {
                    return Err(violation(
                        format!("{field}/id"),
                        format!(
                            "must be 1..={MAX_CUSTOM_EMOJI_ID_LEN} characters of A-Z a-z 0-9 _ . : @ -"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Whether an emoji can currently be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiAvailability {
    /// The emoji can be selected.
    Available,
    /// The emoji cannot be selected (greyed out). Stored favourites and
    /// reactions are kept and can still be removed.
    Unavailable,
}

/// Why an emoji is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiUnavailableReason {
    /// The custom emoji was deleted (or never existed on this server).
    Deleted,
    /// The custom emoji exists but was disabled.
    Disabled,
}

/// An emoji as returned by the server: identity plus availability and display data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EmojiView {
    /// Identity of the emoji.
    pub emoji: EmojiRef,
    /// Current availability. Unicode emoji are always `available`.
    pub availability: EmojiAvailability,
    /// Present only when `availability` is `unavailable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<EmojiUnavailableReason>,
    /// Display shortcode of a custom emoji, without colons; absent for Unicode emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcode: Option<String>,
    /// Image of a custom emoji when one is still served; absent for Unicode
    /// emoji and for deleted custom emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

fn emoji_ref_array() -> RefOr<Schema> {
    ArrayBuilder::new()
        .description(Some(
            "The complete new ordered list: 0..=5 pairwise distinct emoji. An empty \
             list clears the favourites.",
        ))
        .items(Ref::from_schema_name("EmojiRef"))
        .max_items(Some(MAX_FAVORITES))
        .unique_items(true)
        .into()
}

fn emoji_view_array() -> RefOr<Schema> {
    ArrayBuilder::new()
        .description(Some(
            "Ordered favourites, at most 5 and pairwise distinct by `emoji`. \
             Unavailable favourites are kept and flagged, never removed silently.",
        ))
        .items(Ref::from_schema_name("EmojiView"))
        .max_items(Some(MAX_FAVORITES))
        .into()
}

/// Quick reaction settings of a Profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct QuickReactionSettings {
    /// Monotonic revision, `0` for a Profile that never saved settings. It
    /// increases by one whenever the stored favourites change.
    pub revision: u64,
    /// Ordered favourites, at most 5 and pairwise distinct by `emoji`.
    /// Unavailable favourites are kept and flagged, never removed silently.
    #[schema(schema_with = emoji_view_array)]
    pub favorites: Vec<EmojiView>,
}

/// Request body replacing the favourites of a Profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateQuickReactionSettings {
    /// The revision the client last saw. Use `0` for the first save.
    pub expected_revision: u64,
    /// The complete new ordered list: 0..=5 pairwise distinct emoji. An empty
    /// list clears the favourites.
    #[schema(schema_with = emoji_ref_array)]
    pub favorites: Vec<EmojiRef>,
}

impl UpdateQuickReactionSettings {
    /// Check count, uniqueness and identifier formats, reporting every problem.
    pub fn validate(&self) -> Result<(), Vec<FieldViolation>> {
        let mut problems = Vec::new();
        if self.favorites.len() > MAX_FAVORITES {
            problems.push(violation(
                "/favorites",
                format!("at most {MAX_FAVORITES} favorites are allowed"),
            ));
        }
        for (index, emoji) in self.favorites.iter().enumerate() {
            let field = format!("/favorites/{index}");
            if let Err(problem) = emoji.validate(&field) {
                problems.push(problem);
            }
            if self.favorites[..index].contains(emoji) {
                problems.push(violation(field, "duplicate of an earlier favorite"));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// The reaction a Profile has on a Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ReactionState {
    /// The Note the reaction belongs to.
    pub note_id: Uuid,
    /// The selected emoji, or `null` when the Profile has not reacted. The
    /// emoji may be `unavailable`; it can still be removed.
    #[schema(required = true, nullable = true)]
    pub emoji: Option<EmojiView>,
}

/// Request body selecting the reaction of a Profile on a Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutReaction {
    /// The emoji to select; replaces any previous selection.
    pub emoji: EmojiRef,
}

impl PutReaction {
    /// Check the emoji identifier.
    pub fn validate(&self) -> Result<(), Vec<FieldViolation>> {
        self.emoji
            .validate("/emoji")
            .map_err(|problem| vec![problem])
    }
}

fn revision_conflict_code() -> ObjectBuilder {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .enum_values(Some(["revision_conflict"]))
        .description(Some("Always `revision_conflict`."))
}

/// `409` body for a stale `expected_revision`: the common `ErrorBody` fields
/// with `code` fixed to `revision_conflict`, plus the stored revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RevisionConflictBody {
    /// Always `revision_conflict`.
    #[schema(schema_with = revision_conflict_code)]
    pub code: ErrorCode,
    /// Human readable description intended for developers, not end users.
    pub message: String,
    /// Optional per-field problems.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<FieldViolation>,
    /// The revision currently stored; refetch the settings and retry.
    pub current_revision: u64,
}

impl RevisionConflictBody {
    /// Conflict answer carrying the revision that is actually stored.
    pub fn new(current_revision: u64) -> Self {
        Self {
            code: ErrorCode::RevisionConflict,
            message: "expected_revision does not match the stored revision".into(),
            details: Vec::new(),
            current_revision,
        }
    }
}

impl ApiV1Error {
    /// `400 invalid_request` carrying the field violations.
    pub fn invalid_request(details: Vec<FieldViolation>) -> Self {
        Self(ErrorBody {
            code: ErrorCode::InvalidRequest,
            message: "the request failed validation".into(),
            details,
        })
    }
}

/// Get the quick reaction settings of a Profile (stub: always answers 501)
#[utoipa::path(
    get,
    path = "/api/v1/profiles/{profile_nanoid}/quick-reaction-settings",
    operation_id = "getQuickReactionSettings",
    tag = "quick-reaction",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Posting Profile (Emumet profile_nanoid)")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Settings; a never-saved Profile reads as revision 0 with no favorites. Contract only: never returned while the endpoint is a stub", body = QuickReactionSettings),
        (status = 400, description = "Malformed profile_nanoid (invalid_request)", body = ErrorBody),
        (status = 401, description = "Not authenticated (unauthorized)", body = ErrorBody),
        (status = 403, description = "The caller may not act as this Profile (forbidden)", body = ErrorBody),
        (status = 404, description = "Unknown Profile (not_found)", body = ErrorBody),
        (status = 501, description = "Not implemented yet (not_implemented)", body = ErrorBody),
    )
)]
pub async fn get_quick_reaction_settings() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Replace the quick reaction settings of a Profile (stub: always answers 501)
///
/// Full replacement guarded by `expected_revision`. Idempotent retry: when
/// `favorites` equal the stored list the stored settings are returned with
/// `200` and the revision is unchanged, whatever `expected_revision` is.
/// Otherwise a revision mismatch is `409 revision_conflict`.
#[utoipa::path(
    put,
    path = "/api/v1/profiles/{profile_nanoid}/quick-reaction-settings",
    operation_id = "putQuickReactionSettings",
    tag = "quick-reaction",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Posting Profile (Emumet profile_nanoid)")),
    request_body = UpdateQuickReactionSettings,
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Stored settings after the update (new revision), or unchanged settings for an idempotent retry. Contract only: never returned while the endpoint is a stub", body = QuickReactionSettings),
        (status = 400, description = "More than 5 favorites, duplicates, malformed emoji identifiers or profile_nanoid (invalid_request with details)", body = ErrorBody),
        (status = 401, description = "Not authenticated (unauthorized)", body = ErrorBody),
        (status = 403, description = "The caller may not act as this Profile (forbidden)", body = ErrorBody),
        (status = 404, description = "Unknown Profile (not_found)", body = ErrorBody),
        (status = 409, description = "expected_revision is stale (revision_conflict); current_revision tells the stored revision", body = RevisionConflictBody),
        (status = 422, description = "A newly added emoji is unavailable (emoji_unavailable); favorites already stored may stay (details point at the offending /favorites/{index})", body = ErrorBody),
        (status = 501, description = "Not implemented yet (not_implemented)", body = ErrorBody),
    )
)]
pub async fn put_quick_reaction_settings() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Get the reaction of a Profile on a Note (stub: always answers 501)
#[utoipa::path(
    get,
    path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction",
    operation_id = "getNoteReaction",
    tag = "quick-reaction",
    params(
        ("profile_nanoid" = ProfileNanoid, Path, description = "Reacting Profile (Emumet profile_nanoid)"),
        ("note_id" = Uuid, Path, description = "Note the reaction belongs to"),
    ),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Selected emoji, or emoji null when not reacted. Contract only: never returned while the endpoint is a stub", body = ReactionState),
        (status = 400, description = "Malformed profile_nanoid (invalid_request)", body = ErrorBody),
        (status = 401, description = "Not authenticated (unauthorized)", body = ErrorBody),
        (status = 403, description = "The caller may not act as this Profile (forbidden)", body = ErrorBody),
        (status = 404, description = "Unknown Profile or Note (not_found)", body = ErrorBody),
        (status = 501, description = "Not implemented yet (not_implemented)", body = ErrorBody),
    )
)]
pub async fn get_note_reaction() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Select the reaction of a Profile on a Note (stub: always answers 501)
///
/// Idempotent: repeating the same emoji returns `200` with the same state. A
/// different emoji replaces the previous one (1 Profile, 1 Note, 1 reaction).
#[utoipa::path(
    put,
    path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction",
    operation_id = "putNoteReaction",
    tag = "quick-reaction",
    params(
        ("profile_nanoid" = ProfileNanoid, Path, description = "Reacting Profile (Emumet profile_nanoid)"),
        ("note_id" = Uuid, Path, description = "Note the reaction belongs to"),
    ),
    request_body = PutReaction,
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "State after selecting (also for a repeated or replacing request). Contract only: never returned while the endpoint is a stub", body = ReactionState),
        (status = 400, description = "Malformed emoji identifier or profile_nanoid (invalid_request with details)", body = ErrorBody),
        (status = 401, description = "Not authenticated (unauthorized)", body = ErrorBody),
        (status = 403, description = "The caller may not act as this Profile (forbidden)", body = ErrorBody),
        (status = 404, description = "Unknown Profile or Note (not_found)", body = ErrorBody),
        (status = 422, description = "The emoji is unavailable (emoji_unavailable); the previous reaction stays", body = ErrorBody),
        (status = 501, description = "Not implemented yet (not_implemented)", body = ErrorBody),
    )
)]
pub async fn put_note_reaction() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Remove the reaction of a Profile on a Note (stub: always answers 501)
///
/// Idempotent and independent of emoji availability: answers `200` with
/// `emoji: null` also when nothing was selected, and also when the selected
/// emoji has become unavailable.
#[utoipa::path(
    delete,
    path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction",
    operation_id = "deleteNoteReaction",
    tag = "quick-reaction",
    params(
        ("profile_nanoid" = ProfileNanoid, Path, description = "Reacting Profile (Emumet profile_nanoid)"),
        ("note_id" = Uuid, Path, description = "Note the reaction belongs to"),
    ),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Unreacted state (emoji null), including a repeated delete. Contract only: never returned while the endpoint is a stub", body = ReactionState),
        (status = 400, description = "Malformed profile_nanoid (invalid_request)", body = ErrorBody),
        (status = 401, description = "Not authenticated (unauthorized)", body = ErrorBody),
        (status = 403, description = "The caller may not act as this Profile (forbidden)", body = ErrorBody),
        (status = 404, description = "Unknown Profile or Note (not_found); never because no reaction exists", body = ErrorBody),
        (status = 501, description = "Not implemented yet (not_implemented)", body = ErrorBody),
    )
)]
pub async fn delete_note_reaction() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Quick reaction routes, registered once through `routes!` (stubs take no
/// extractors so they answer 501 for any request).
pub fn router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            get_quick_reaction_settings,
            put_quick_reaction_settings
        ))
        .routes(routes!(
            get_note_reaction,
            put_note_reaction,
            delete_note_reaction
        ))
}
