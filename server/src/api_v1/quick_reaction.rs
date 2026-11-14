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
//!   Selecting an unavailable emoji answers `422 emoji_unavailable`; this is
//!   checked before the no-op rule, so repeating the already selected emoji
//!   after it became unavailable is also `422` (the selection stays).
//! * Reaction `DELETE` always answers `200` with the unreacted state, also when
//!   nothing was selected (retry safe) and **also when the selected emoji has
//!   become unavailable**: removal never depends on availability.
//! * Reaction updates carry no revision: the resource holds one value, so
//!   concurrent requests resolve last-write-wins.
//! * Validation errors (`invalid_request`) are reported with per-field
//!   [`FieldViolation`]s whose `field` is a JSON pointer into the body.
//!
//! # External custom emoji (D09 / `remote-emoji-reuse-api-contract`)
//!
//! A custom emoji that comes from another server is an *external* emoji.
//! [`EmojiRef`] is unchanged (the identity stays an opaque, server assigned id
//! that differs per origin, so two emoji with the same shortcode on different
//! servers are different emoji). The extra facts live in [`EmojiView`]:
//!
//! * `origin`: the originating domain. Present exactly for external emoji;
//!   absent for Unicode and for custom emoji of this server. This is what the
//!   UI uses for the globe mark that tells same-named emoji apart, on reaction
//!   chips (`EmojiReactionCount.view`, S8) and on the own selection alike.
//! * `image_state`: `present` or `missing`, independent of `availability`. A
//!   `missing` image is shown as `:shortcode:` and the emoji can still be
//!   selected while it is `available` (the image just could not be fetched, or
//!   the origin deleted it and the local copy expired). This is the only place
//!   where "the image is gone" is expressed: an external emoji deleted at its
//!   origin (or whose cached image expired) is `available` + `missing`, never
//!   `unavailable` with reason `deleted` (decision d09-q07; the UI shows it as
//!   `:name:` and it can be pressed from an existing chip like any other
//!   external emoji). Whether it may be sent is decided by `availability`
//!   and the usage rules below, not by the image.
//! * `usage`: `existing_reaction_only` (external emoji: usable only through an
//!   existing chip of the Note) or `anywhere`. External emoji are never offered
//!   in the picker, in recents or as favourites.
//!
//! Settings `PUT` refuses an external emoji with `422 emoji_usage_not_allowed`
//! (`details` point at `/favorites/{index}`) and no settings answer ever lists
//! one. Reaction `PUT` applies this order (the first matching rule answers):
//!
//! 1. availability: an unavailable emoji is `422 emoji_unavailable`. This
//!    covers a deleted or disabled custom emoji of this server and an external
//!    emoji this server disabled (`disabled`); it does **not** cover an
//!    external emoji that was deleted at its origin or whose image expired
//!    (those stay `available` with `image_state: missing`);
//! 2. the emoji is already the current selection: `200`, no-op;
//! 3. external emoji and the Note has no chip of that same emoji:
//!    `422 emoji_usage_not_allowed` (also for an external emoji deleted at its
//!    origin: it is only reachable through a chip that is still there);
//! 4. external emoji and the Profile already reacted with another emoji:
//!    `409 conflict` (no direct replacement, the current reaction stays; the
//!    client removes it with `DELETE` first);
//! 5. select the emoji: `200`.
//!
//! So an external emoji deleted at its origin whose chip is still on the Note
//! is selected with `200` (the answer shows `image_state: missing`), and
//! repeating it while selected is the rule 2 no-op. Settings `PUT` refuses it
//! like any external emoji (`emoji_usage_not_allowed`, not `emoji_unavailable`).
//!
//! Rules 3 and 4 only concern `usage: existing_reaction_only`; Unicode and
//! same-server emoji keep the replacement semantics described above. Removing
//! the only reaction of an external emoji can remove its chip, so selecting it
//! again afterwards may be rule 3. `DELETE` is unchanged. Nothing of D03 q09
//! (delayed unreaction call) or q10 (suppressed repeat notifications) appears
//! in any response, so they need no field.
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

use super::personal_domain::ControlDomain;
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
        /// At most 32 code points, no Unicode White_Space or control characters
        /// and at least one non-ASCII code point; the pattern spells out exactly
        /// the characters the server rejects (the exact emoji set is checked by
        /// the server; skin tones and ZWJ sequences are distinct emoji).
        #[schema(
            min_length = 1,
            max_length = 32,
            pattern = r"^[^\x00-\x20\x7F-\xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]*[^\x00-\xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000][^\x00-\x20\x7F-\xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]*$",
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
    /// The emoji can be selected (subject to the usage rules). An external
    /// emoji deleted at its origin, or whose image expired, stays `available`;
    /// only its `image_state` becomes `missing`.
    Available,
    /// The emoji cannot be selected (greyed out). Stored favourites and
    /// reactions are kept and can still be removed. For an external emoji this
    /// means this server disabled it (`disabled`), never that its image is gone.
    Unavailable,
}

/// Whether the image of a custom emoji can be shown. Independent of
/// [`EmojiAvailability`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiImageState {
    /// `image_url` is served and the image can be shown.
    Present,
    /// No image can be shown (never fetched, fetch failed, or deleted at the
    /// origin and expired locally): show `:shortcode:` instead. `image_url` is
    /// absent. A missing image does not change `availability` by itself: an
    /// `available` emoji that satisfies the usage rules can still be selected
    /// (an external emoji deleted at its origin looks like this and is
    /// selectable from an existing chip).
    Missing,
}

/// Where a custom emoji may be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiUsage {
    /// Only by pressing an existing reaction chip of the same emoji on a Note
    /// (external emoji). Never in the picker, recents or favourites.
    ExistingReactionOnly,
    /// Anywhere an emoji can be chosen.
    Anywhere,
}

/// Why an emoji is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiUnavailableReason {
    /// The custom emoji of this server was deleted (or never existed on this
    /// server). Never used for an external emoji: deletion at the origin is
    /// expressed by `image_state: missing` only.
    Deleted,
    /// The custom emoji exists but this server disabled it (also possible for
    /// an external emoji).
    Disabled,
}

/// An emoji as returned by the server: identity plus availability and display data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EmojiView {
    /// Identity of the emoji.
    pub emoji: EmojiRef,
    /// Current availability. Unicode emoji are always `available`. An external
    /// emoji is `unavailable` only when this server disabled it; deletion at
    /// the origin is `image_state: missing` while it stays `available`.
    pub availability: EmojiAvailability,
    /// Present only when `availability` is `unavailable`. Never `deleted` for an
    /// external emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<EmojiUnavailableReason>,
    /// Display shortcode of a custom emoji, without colons; absent for Unicode emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcode: Option<String>,
    /// Image of a custom emoji when one is still served; absent for Unicode
    /// emoji and for deleted custom emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// Originating domain of an external custom emoji (same canonical host
    /// form as the E8 domain controls). Absent for Unicode emoji and for custom
    /// emoji of this server, so its presence is the external mark (globe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ControlDomain>,
    /// Whether the image can be shown; independent of `availability`. Present
    /// for custom emoji, absent for Unicode emoji. `present` goes with
    /// `image_url`, `missing` means show `:shortcode:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_state: Option<EmojiImageState>,
    /// Where the emoji may be used. Present for custom emoji (an external one
    /// is `existing_reaction_only`); absent means `anywhere` (Unicode emoji).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<EmojiUsage>,
}

impl EmojiView {
    /// The consistency rules every `EmojiView` the server sends obeys.
    ///
    /// Unicode emoji carry no custom emoji data; an `unavailable_reason` goes
    /// with `unavailable` only; `image_state` and `image_url` agree; an external
    /// emoji (with `origin`) is `existing_reaction_only` and is never `unavailable`
    /// because of deletion (`unavailable_reason` is not `deleted`: the image state
    /// carries that); a custom emoji whose image is `missing` keeps a non-empty
    /// `shortcode` to show as `:name:`.
    pub fn check_consistency(&self) -> Result<(), String> {
        if matches!(self.availability, EmojiAvailability::Available)
            == self.unavailable_reason.is_some()
        {
            return Err("unavailable_reason must be present exactly when unavailable".into());
        }
        if matches!(self.emoji, EmojiRef::Unicode { .. }) {
            let custom_only = self.origin.is_some()
                || self.image_state.is_some()
                || self.shortcode.is_some()
                || self.image_url.is_some()
                || self.usage == Some(EmojiUsage::ExistingReactionOnly);
            if custom_only || self.availability != EmojiAvailability::Available {
                return Err("a Unicode emoji is always available and has no custom data".into());
            }
            return Ok(());
        }
        match self.image_state {
            Some(EmojiImageState::Present) if self.image_url.is_none() => {
                return Err("image_state present needs image_url".into());
            }
            Some(EmojiImageState::Missing) if self.image_url.is_some() => {
                return Err("image_state missing must not carry image_url".into());
            }
            Some(EmojiImageState::Missing)
                if self.shortcode.as_deref().is_none_or(str::is_empty) =>
            {
                return Err("image_state missing needs a shortcode to show as :name:".into());
            }
            _ => {}
        }
        if self.origin.is_some() && self.usage != Some(EmojiUsage::ExistingReactionOnly) {
            return Err("an external emoji must be existing_reaction_only".into());
        }
        if self.origin.is_some() && self.unavailable_reason == Some(EmojiUnavailableReason::Deleted)
        {
            return Err(
                "an external emoji deleted at its origin is available with image_state missing, \
                 not unavailable/deleted"
                    .into(),
            );
        }
        Ok(())
    }
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
    /// An external emoji (`usage: existing_reaction_only`) is never listed.
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
    /// emoji may be `unavailable`, or external (`origin` set); it can still be
    /// removed.
    #[schema(required = true, nullable = true)]
    pub emoji: Option<EmojiView>,
}

/// Request body selecting the reaction of a Profile on a Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutReaction {
    /// The emoji to select; replaces any previous selection, except that an
    /// external emoji (`usage: existing_reaction_only`) never replaces: it is
    /// refused with `409` while another reaction is selected.
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
            domain_block: None,
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
        (status = 422, description = "A newly added emoji is unavailable (emoji_unavailable), or is an external emoji that may not be a favorite (emoji_usage_not_allowed, usage existing_reaction_only); favorites already stored may stay (details point at the offending /favorites/{index}). Unavailable is reported before usage", body = ErrorBody),
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
/// different emoji replaces the previous one (1 Profile, 1 Note, 1 reaction),
/// except for external emoji (`usage: existing_reaction_only`, see the module
/// docs). Rules, first match answers: 1 an unavailable emoji is `422
/// emoji_unavailable` (a deleted or disabled custom emoji of this server, or an
/// external emoji this server disabled; an external emoji deleted at its origin
/// is still available), even when it is the emoji already selected (the
/// selection then stays and can be removed with `DELETE`); 2 the emoji equals
/// the current selection: `200` no-op; 3 external emoji without a chip of the
/// same emoji on the Note: `422 emoji_usage_not_allowed` (also when it was
/// deleted at its origin; with the chip still there it is selected with `200`
/// and shown as `:name:`); 4 external emoji while
/// the Profile reacted with another emoji: `409 conflict` (no direct
/// replacement, the current reaction stays); 5 select: `200`.
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
        (status = 403, description = "Forbidden: caller may not act as Profile, or E8 domain block refuses the new emoji operation. Block uses optional ErrorBody.domain_block (EmojiReactionDomainBlock); explicit unblock required first. Mute adds no restriction. Existing S3 error shape remains valid.", body = ErrorBody),
        (status = 404, description = "Unknown Profile or Note (not_found)", body = ErrorBody),
        (status = 409, description = "An external emoji (usage existing_reaction_only) was requested while the Profile already reacted with another emoji (conflict); the current reaction stays, DELETE it first", body = ErrorBody),
        (status = 422, description = "The emoji is unavailable (emoji_unavailable, checked before the repeat no-op rule; not for an external emoji merely deleted at its origin), or an external emoji has no chip on the Note (emoji_usage_not_allowed, checked after it, also for one deleted at its origin); the current reaction stays", body = ErrorBody),
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
