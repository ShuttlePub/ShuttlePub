//! S8 / D08 personal domain content contract. **Contract only**: every route
//! answers 501; no filtering, policy storage, normalisation or enforcement runs.
//!
//! Emumet E8 owns Profile × domain policy. See its openapi.json
//! DomainControlResponse/DomainControlMode and the host's
//! intents/emumet/features/personal-domain-controls/acceptance.md (E8/D8P).
//! Only verified canonical actor-ID hosts are matched, exactly (never a suffix).
//! Account discovery, nested-quote attribution, historical notification/push
//! delivery, Follow and existing-operation Undo are deliberately unspecified.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use super::quick_reaction::{EmojiRef, ProfileNanoid};
use super::{ApiV1Error, ErrorBody, PageParams};

/// E8 DomainControlMode, verbatim: mute suppresses display only; block also
/// refuses new operations in both directions. Never a new ShuttlePub setting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomainControlMode {
    Mute,
    Block,
}

/// E8 DomainControlResponse.domain, not a second normalizer. Already normalized
/// lowercase ASCII/IDNA A-label host; UTS #46 non-transitional; remove one trailing
/// dot. Exact match: example.com does not include sub.example.com. E8 rejects
/// invalid inputs with 422. Match only Emumet D8P ControlDomain::from_actor_id's
/// verified canonical actor-ID host, never display acct, username or asserted domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(transparent)]
pub struct ControlDomain(pub String);

/// Immutable observation of E8 DomainControlResponse, not a local policy copy.
/// No created_at/updated_at needed for content presentation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DomainControlRef {
    /// E8 profile_id (Profile nanoid); same value as path profile_nanoid and
    /// ProfileResponse.id. Never Account ID, actor URL or nullable username.
    pub profile_id: ProfileNanoid,
    pub domain: ControlDomain,
    pub mode: DomainControlMode,
    /// E8 int64 revision, starts at 1. Create/replace/delete/recreate advance it;
    /// tombstones prevent reuse. Not S3 settings revision or a reveal revision.
    #[schema(minimum = 1)]
    pub revision: i64,
}

/// Block-only projection of E8 mode; not an additional policy mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomainBlockMode {
    Block,
}

/// The blocking E8 policy snapshot. Changing this requires an explicit Emumet
/// domain-controls update; a post operation or reveal never unblocks implicitly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockingDomainControlRef {
    pub profile_id: ProfileNanoid,
    pub domain: ControlDomain,
    pub mode: DomainBlockMode,
    /// The same E8 int64 monotonic revision, never minted by ShuttlePub.
    #[schema(minimum = 1)]
    pub revision: i64,
}

/// Minimal content projection for this contract, not a full author/media DTO.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteContent {
    pub note_id: Uuid,
    pub content: String,
}

/// A feed contains only visible items. For mute AND block, omit an entire
/// third-party quote (including the quoter's comment) or pure repost when its
/// immediate target is suppressed. Do not mute/block that third party or their
/// unrelated posts. No hidden variant, placeholder, suppressed count or note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FeedItem {
    Post {
        note: NoteContent,
    },
    Reply {
        note: NoteContent,
    },
    Quote {
        note: NoteContent,
        quoted_note: NoteContent,
    },
    Repost {
        note_id: Uuid,
        reposted_note: NoteContent,
    },
}

/// S8 collection cursor only. Closed metadata prevents hidden-count/annotation
/// extensions; opaque cursors must not disclose suppressed identities or counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentPageInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Profile-scoped timeline: suppressed items leave no trace, including counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TimelineResponse {
    pub profile_id: ProfileNanoid,
    pub items: Vec<FeedItem>,
    pub page: ContentPageInfo,
}

/// Post search only (not Account search). No suppression annotation or count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteSearchResponse {
    pub profile_id: ProfileNanoid,
    pub items: Vec<FeedItem>,
    pub page: ContentPageInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentNotification {
    pub notification_id: Uuid,
    pub item: FeedItem,
    pub unread: bool,
}

/// Only visible notifications contribute to unread_count. No hidden marker,
/// count, explanation or push/history recall guarantee is exposed here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentNotificationsResponse {
    pub profile_id: ProfileNanoid,
    pub items: Vec<ContentNotification>,
    pub unread_count: u64,
    pub page: ContentPageInfo,
}

/// Independent content gates. Reveal does not clear CW/sensitive, deletion or
/// viewing permission. A restricted response carries no gated content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContentRestriction {
    Cw,
    Sensitive,
    Deleted,
    Visibility,
}

/// One embedded target only. Revealing an outer quote never reveals
/// this target. No decision about attribution through nested quotes is made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddedNotePresentation {
    Visible {
        note: NoteContent,
    },
    Hidden {
        mode: DomainControlMode,
        domain: ControlDomain,
        /// E8 revision, starts at 1, monotonically increases, never reused.
        #[schema(minimum = 1)]
        revision: i64,
    },
    Restricted {
        reason: ContentRestriction,
    },
}

/// Explicitly revealed item. Boost-wrapper direct reveal is undecided and not
/// part of S8 (the original note URL uses ordinary single-note reveal).
/// Operations concern its own author/target, NOT the
/// domain of an embedded quote. The unblocked quoter remains actionable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectItem {
    Post {
        note: NoteContent,
    },
    Reply {
        note: NoteContent,
    },
    Quote {
        note: NoteContent,
        quoted_note: EmbeddedNotePresentation,
    },
}

/// Domain-only operation permission; allowed does not bypass D13 or any other
/// permission. Mute adds no operation restriction. Block is not cleared by reveal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum DomainOperationPermission {
    Allowed,
    Blocked { control: BlockingDomainControlRef },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostOperationAvailability {
    pub reply: DomainOperationPermission,
    pub quote: DomainOperationPermission,
    pub repost: DomainOperationPermission,
    pub like: DomainOperationPermission,
    pub emoji_reaction: DomainOperationPermission,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmojiReactionCount {
    pub emoji: EmojiRef,
    pub count: u64,
}

/// Accepted historical likes/emoji remain counted after block; rejected new
/// operations add nothing. No automatic deletion or Undo, no block annotation.
/// Independent legitimate removal can still change counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionCounts {
    pub likes: u64,
    pub emoji: Vec<EmojiReactionCount>,
}

/// Direct URL presentation. Hidden contains ONLY state, E8 mode, domain and
/// revision: no body, author details, media, CW text, mention or embedded data.
/// Restricted remains content-free even after an explicit single-note reveal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotePresentation {
    Hidden {
        mode: DomainControlMode,
        domain: ControlDomain,
        /// E8 revision, not a session/reveal token.
        #[schema(minimum = 1)]
        revision: i64,
    },
    Visible {
        item: DirectItem,
        operations: PostOperationAvailability,
        reaction_counts: ReactionCounts,
    },
    Restricted {
        reason: ContentRestriction,
        operations: PostOperationAvailability,
    },
}

/// Profile scoped direct view. Initial GET is hidden for a controlled domain.
/// POST reveal is explicit and applies only to this note in the current page.
/// Never persist or sync reveal, never change E8 revision. Hide discards the
/// page-local result; reload/revisit uses GET and hides again. Reveal never
/// changes another note, embedded content, block, CW/sensitive or viewing rights.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectNoteResponse {
    pub profile_id: ProfileNanoid,
    pub presentation: NotePresentation,
}

/// Reply/quote input only; success-creation semantics are outside S8.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostComment {
    #[schema(min_length = 1)]
    pub content: String,
}

/// Internal acceptance decision, not an ActivityPub inbox wire response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RejectedOutcome {
    Rejected,
}

/// Operation identity, distinct on the wire from every other post action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplyOperation {
    Reply,
}

/// Per-operation reply block refusal details. Explicit unblock in Emumet is
/// required first; mute never produces this result. D13 cannot override block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyDomainBlock {
    pub operation: ReplyOperation,
    pub control: BlockingDomainControlRef,
}

/// ShuttlePub INTERNAL inbound reply acceptance result contract, NOT a wire
/// inbox/API or an ActivityPub HTTP status. Refuse NEW actions from this domain
/// to this Profile, even if D13 permits the action. No rejection on mute alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InboundReplyRejection {
    pub outcome: RejectedOutcome,
    pub rejected: ReplyDomainBlock,
}

/// Operation identity, distinct on the wire from every other post action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuoteOperation {
    Quote,
}

/// Per-operation quote block refusal details. Explicit unblock in Emumet is
/// required first; mute never produces this result. D13 cannot override block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuoteDomainBlock {
    pub operation: QuoteOperation,
    pub control: BlockingDomainControlRef,
}

/// ShuttlePub INTERNAL inbound quote acceptance result contract, NOT a wire
/// inbox/API or an ActivityPub HTTP status. Refuse NEW actions from this domain
/// to this Profile, even if D13 permits the action. No rejection on mute alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InboundQuoteRejection {
    pub outcome: RejectedOutcome,
    pub rejected: QuoteDomainBlock,
}

/// Operation identity, distinct on the wire from every other post action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepostOperation {
    Repost,
}

/// Per-operation repost block refusal details. Explicit unblock in Emumet is
/// required first; mute never produces this result. D13 cannot override block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RepostDomainBlock {
    pub operation: RepostOperation,
    pub control: BlockingDomainControlRef,
}

/// ShuttlePub INTERNAL inbound repost acceptance result contract, NOT a wire
/// inbox/API or an ActivityPub HTTP status. Refuse NEW actions from this domain
/// to this Profile, even if D13 permits the action. No rejection on mute alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InboundRepostRejection {
    pub outcome: RejectedOutcome,
    pub rejected: RepostDomainBlock,
}

/// Operation identity, distinct on the wire from every other post action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LikeOperation {
    Like,
}

/// Per-operation like block refusal details. Explicit unblock in Emumet is
/// required first; mute never produces this result. D13 cannot override block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LikeDomainBlock {
    pub operation: LikeOperation,
    pub control: BlockingDomainControlRef,
}

/// ShuttlePub INTERNAL inbound like acceptance result contract, NOT a wire
/// inbox/API or an ActivityPub HTTP status. Refuse NEW actions from this domain
/// to this Profile, even if D13 permits the action. No rejection on mute alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InboundLikeRejection {
    pub outcome: RejectedOutcome,
    pub rejected: LikeDomainBlock,
}

/// Operation identity, distinct on the wire from every other post action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmojiReactionOperation {
    EmojiReaction,
}

/// Per-operation emoji_reaction block refusal details. Explicit unblock in Emumet is
/// required first; mute never produces this result. D13 cannot override block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmojiReactionDomainBlock {
    pub operation: EmojiReactionOperation,
    pub control: BlockingDomainControlRef,
    pub emoji: EmojiRef,
}

/// ShuttlePub INTERNAL inbound emoji_reaction acceptance result contract, NOT a wire
/// inbox/API or an ActivityPub HTTP status. Refuse NEW actions from this domain
/// to this Profile, even if D13 permits the action. No rejection on mute alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InboundEmojiReactionRejection {
    pub outcome: RejectedOutcome,
    pub rejected: EmojiReactionDomainBlock,
}

/// Optional typed extension on common 403 forbidden ErrorBody. Each variant is
/// a separate operation contract. Like and emoji have distinct wire values;
/// only emoji carries EmojiRef, exactly the S3 PUT request's identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum OutboundDomainBlock {
    Reply(ReplyDomainBlock),
    Quote(QuoteDomainBlock),
    Repost(RepostDomainBlock),
    Like(LikeDomainBlock),
    EmojiReaction(EmojiReactionDomainBlock),
}

/// Omit controlled items for this Profile only, without trace.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    get, path = "/api/v1/profiles/{profile_nanoid}/timeline",
    operation_id = "getPersonalTimeline", tag = "personal-domain-content",
    params(PageParams, ("profile_nanoid" = ProfileNanoid, Path)),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub", body = TimelineResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Caller cannot act as this Profile; domain controls suppress items rather than deny the collection.", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn get_personal_timeline() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Post search only; mute/block suppression leaves no trace.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    get, path = "/api/v1/profiles/{profile_nanoid}/notes/search",
    operation_id = "searchPersonalNotes", tag = "personal-domain-content",
    params(PageParams, ("profile_nanoid" = ProfileNanoid, Path), ("q" = String, Query, description = "Post search text")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub", body = NoteSearchResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Caller cannot act as this Profile; domain controls suppress items rather than deny the collection.", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn search_personal_notes() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Suppressed notifications do not contribute to unread count.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    get, path = "/api/v1/profiles/{profile_nanoid}/notifications",
    operation_id = "getPersonalContentNotifications", tag = "personal-domain-content",
    params(PageParams, ("profile_nanoid" = ProfileNanoid, Path)),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub", body = ContentNotificationsResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Caller cannot act as this Profile; domain controls suppress items rather than deny the collection.", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn get_personal_content_notifications() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Direct URL: initially hidden, including entire third-party quotes.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    get, path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}",
    operation_id = "getPersonalNote", tag = "personal-domain-content",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Emumet Profile nanoid"), ("note_id" = Uuid, Path, description = "ShuttlePub NoteId")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub", body = DirectNoteResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Caller cannot act as Profile or lacks viewing rights. Domain mute/block alone does not forbid single-note reveal.", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn get_personal_note() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Explicit one-note, current-page-only reveal. Never persist, unblock or bypass independent gates.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    post, path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reveal",
    operation_id = "revealPersonalNote", tag = "personal-domain-content",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Emumet Profile nanoid"), ("note_id" = Uuid, Path, description = "ShuttlePub NoteId")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub", body = DirectNoteResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Caller cannot act as Profile or lacks viewing rights. Domain mute/block alone does not forbid single-note reveal.", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn reveal_personal_note() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// New outbound reply: block refusal contract only; successful creation is outside S8.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    post, path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/replies",
    operation_id = "createPersonalReply", tag = "personal-domain-content",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Emumet Profile nanoid"), ("note_id" = Uuid, Path, description = "ShuttlePub NoteId")),
    security(("bearer_auth" = [])),
    request_body = PostComment,
    responses(
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden; outbound block uses operation-specific domain_block details in ErrorBody, never auto-unblocks", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn create_personal_reply() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// New outbound quote: block refusal contract only; successful creation is outside S8.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    post, path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/quotes",
    operation_id = "createPersonalQuote", tag = "personal-domain-content",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Emumet Profile nanoid"), ("note_id" = Uuid, Path, description = "ShuttlePub NoteId")),
    security(("bearer_auth" = [])),
    request_body = PostComment,
    responses(
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden; outbound block uses operation-specific domain_block details in ErrorBody, never auto-unblocks", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn create_personal_quote() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// New outbound pure repost: block refusal contract only; successful creation is outside S8.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    post, path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reposts",
    operation_id = "createPersonalRepost", tag = "personal-domain-content",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Emumet Profile nanoid"), ("note_id" = Uuid, Path, description = "ShuttlePub NoteId")),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden; outbound block uses operation-specific domain_block details in ErrorBody, never auto-unblocks", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn create_personal_repost() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// New outbound like (not emoji): block refusal contract only; success semantics are outside S8.
///
/// Contract only: no real filtering, enforcement or posting. Always 501.
#[utoipa::path(
    put, path = "/api/v1/profiles/{profile_nanoid}/notes/{note_id}/like",
    operation_id = "putPersonalLike", tag = "personal-domain-content",
    params(("profile_nanoid" = ProfileNanoid, Path, description = "Emumet Profile nanoid"), ("note_id" = Uuid, Path, description = "ShuttlePub NoteId")),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden; outbound block uses operation-specific domain_block details in ErrorBody, never auto-unblocks", body = ErrorBody),
        (status = 404, description = "Not found", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
    )
)]
pub async fn put_personal_like() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// All S8 routes registered once; no extractors on contract stubs.
pub fn router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_personal_timeline))
        .routes(routes!(search_personal_notes))
        .routes(routes!(get_personal_content_notifications))
        .routes(routes!(get_personal_note))
        .routes(routes!(reveal_personal_note))
        .routes(routes!(create_personal_reply))
        .routes(routes!(create_personal_quote))
        .routes(routes!(create_personal_repost))
        .routes(routes!(put_personal_like))
}
