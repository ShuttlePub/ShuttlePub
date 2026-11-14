//! D04 note search **author lookup** contract (`note-search-author-lookup-api-contract`).
//!
//! Two helper operations for the search UI's `from:@acct` input (ui-spec §2):
//!
//! * `resolveNoteSearchAuthor`: turn one typed acct into the author's canonical
//!   actor ID URL (the `author` of `searchPersonalNotes`);
//! * `suggestNoteSearchAuthors`: at most five author candidates while the user types.
//!
//! **Contract only (UI first).** This module owns types, pure input checks,
//! utoipa metadata and explicit `501 Not Implemented` stubs. No handler touches a
//! database, the network, a background fetcher or an authentication service, and
//! the server never performs an external lookup (WebFinger, actor fetch),
//! synchronously or asynchronously: that is part of the contract, not a missing feature.
//!
//! Decisions (grill records `d04-q13-author-suggestion-scope`,
//! `d04-q14-acct-resolution-basis`, `d04-q15-host-omitted-acct`):
//!
//! * The target set is the authors ShuttlePub has already fetched notes of AND the
//!   viewing Profile may publicly refer to. Emumet's shared display cache is not
//!   used (it would leak fetch history caused by other Profiles' block/mute).
//! * Suggestions never contain authors the viewing Profile mutes or blocks
//!   (individually or by domain).
//! * An acct resolves only through a username this server has confirmed or a
//!   verified remote acct mapping; an acct merely derived for display
//!   (`preferred_username` + actor host, not WebFingered) never resolves.
//! * `@alice` without a host is this server's `alice`.

use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::personal_domain::ActorUrl;
use super::quick_reaction::ProfileNanoid;
use super::{ApiV1Error, ErrorBody, FieldViolation};

/// Maximum length of the normalised acct (`user` or `user@host`) and of the
/// suggestion prefix, in characters (Unicode scalar values).
pub const AUTHOR_LOOKUP_MAX_CHARS: usize = 512;
/// Maximum length of the raw `acct` / `prefix` value: the normalised maximum plus the
/// one optional leading `@`. Longer values are rejected before normalisation.
pub const AUTHOR_LOOKUP_MAX_RAW_CHARS: usize = AUTHOR_LOOKUP_MAX_CHARS + 1;
/// Maximum number of suggestions in one response. There is no paging and no total.
pub const AUTHOR_SUGGESTION_MAX_ITEMS: usize = 5;
/// Maximum length of an `icon_url`, in characters.
pub const AUTHOR_ICON_URL_MAX_CHARS: usize = 2048;

const MAX_HOST_OCTETS: usize = 253;
const MAX_LABEL_OCTETS: usize = 63;

fn fail(field: &str, message: &str) -> Vec<FieldViolation> {
    vec![FieldViolation {
        field: field.to_owned(),
        message: message.to_owned(),
    }]
}

/// Unicode bidirectional formatting controls (embeddings, overrides, isolates,
/// directional marks and the Arabic letter mark).
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Characters of an acct that are URL syntax rather than acct syntax. They reject
/// `from:` / `acct:` prefixes (`:`), actor URLs and paths (`/`, `?`, `#`), and
/// percent-escapes (`%`). A host therefore cannot carry a port.
fn is_url_syntax(c: char) -> bool {
    matches!(
        c,
        '/' | '\\' | '?' | '#' | ':' | '<' | '>' | '[' | ']' | '%'
    )
}

/// A validated, normalised acct: the `username` exactly as typed (never lowercased)
/// and an optional host. `host: None` means "this server" (d04-q15). The leading
/// `@` of the input is not part of the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorAcct {
    pub username: String,
    /// Lowercase ASCII (IDNA A-label) DNS host, or `None` for this server.
    pub host: Option<String>,
}

impl AuthorAcct {
    /// `user@host`, or `user` when the host was omitted.
    pub fn to_acct_string(&self) -> String {
        match &self.host {
            Some(host) => format!("{}@{host}", self.username),
            None => self.username.clone(),
        }
    }
}

/// Normalise a host to lowercase ASCII (UTS #46 / IDNA) and check the DNS limits:
/// at most 253 octets, labels of 1..=63 octets made of `a-z 0-9 -` that neither
/// start nor end with `-`. IP addresses (including numeric shorthand such as
/// `1.2.3` or `0x7f.1`), an empty label and a trailing dot are rejected.
fn normalize_host(host: &str) -> Result<String, &'static str> {
    let domain = match url::Host::parse(host) {
        Ok(url::Host::Domain(domain)) => domain,
        Ok(_) => return Err("host must be a DNS name, not an IP address"),
        Err(_) => return Err("host is not a valid domain name"),
    };
    if domain.is_empty() || domain.len() > MAX_HOST_OCTETS {
        return Err("host must be 1..=253 octets");
    }
    for label in domain.split('.') {
        let valid_chars = label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if label.is_empty()
            || label.len() > MAX_LABEL_OCTETS
            || !valid_chars
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return Err(
                "host labels must be 1..=63 octets of a-z 0-9 - without a leading or trailing hyphen",
            );
        }
    }
    Ok(domain)
}

/// Pure input check of `resolveNoteSearchAuthor`'s single `acct` value, always
/// reported as `400 invalid_request` with `details.field = acct`.
///
/// Accepted: `user@host`, `@user@host` and, without a host, `user` / `@user` (this
/// server's username). Exactly one leading `@` is removed. The username is kept
/// verbatim (no lowercasing, no Unicode normalisation) and no host is ever guessed;
/// the host is normalised to lowercase ASCII. Rejected: an empty value, more than
/// 513 raw characters, whitespace, control or bidi control characters, U+FFFD
/// (invalid UTF-8 percent-encoding), URL syntax (`/ \ ? # : < > [ ] %`, so no `from:` /
/// `acct:` prefix and no actor URL), a missing username or host, more than one
/// `@` separator, an invalid host, and a normalised acct over 512 characters.
pub fn normalize_acct(raw: &str) -> Result<AuthorAcct, Vec<FieldViolation>> {
    const FIELD: &str = "acct";
    if raw.is_empty() {
        return Err(fail(FIELD, "must not be empty"));
    }
    if raw.chars().count() > AUTHOR_LOOKUP_MAX_RAW_CHARS {
        return Err(fail(FIELD, "must be at most 513 characters"));
    }
    if raw.chars().any(char::is_whitespace) {
        return Err(fail(FIELD, "must not contain whitespace"));
    }
    if raw
        .chars()
        .any(|c| c.is_control() || is_bidi_control(c) || c == '\u{FFFD}')
    {
        return Err(fail(
            FIELD,
            "must not contain control, bidi control or invalid characters",
        ));
    }
    if raw.chars().any(is_url_syntax) {
        return Err(fail(
            FIELD,
            "must be an acct (user@host), not a URL or a prefixed expression such as from:",
        ));
    }
    let body = raw.strip_prefix('@').unwrap_or(raw);
    let mut parts = body.split('@');
    let (username, host) = (parts.next().unwrap_or_default(), parts.next());
    if parts.next().is_some() {
        return Err(fail(
            FIELD,
            "must contain at most one @ between user and host",
        ));
    }
    if username.is_empty() {
        return Err(fail(FIELD, "user part must not be empty"));
    }
    let host = match host {
        None => None,
        Some("") => return Err(fail(FIELD, "host part must not be empty")),
        Some(host) => Some(normalize_host(host).map_err(|message| fail(FIELD, message))?),
    };
    let acct = AuthorAcct {
        username: username.to_owned(),
        host,
    };
    if acct.to_acct_string().chars().count() > AUTHOR_LOOKUP_MAX_CHARS {
        return Err(fail(
            FIELD,
            "normalized acct must be at most 512 characters",
        ));
    }
    Ok(acct)
}

/// A validated suggestion prefix: the text after removing one leading `@`, kept
/// exactly as typed. Matching folds it with [`fold_for_match`]; this value is never
/// echoed back in a response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorPrefix(pub String);

/// Pure input check of `suggestNoteSearchAuthors`' single `prefix` value, always
/// reported as `400 invalid_request` with `details.field = prefix`.
///
/// One leading `@` is removed; the rest must be 1..=512 characters. Spaces are
/// allowed (display names contain them); control characters and U+FFFD are not.
/// `%`, `_`, `\` and every other character are literal: there is no wildcard,
/// regular expression or partial (non-prefix) match.
pub fn normalize_prefix(raw: &str) -> Result<AuthorPrefix, Vec<FieldViolation>> {
    const FIELD: &str = "prefix";
    if raw.is_empty() {
        return Err(fail(FIELD, "must not be empty"));
    }
    if raw.chars().count() > AUTHOR_LOOKUP_MAX_RAW_CHARS {
        return Err(fail(FIELD, "must be at most 513 characters"));
    }
    let body = raw.strip_prefix('@').unwrap_or(raw);
    let len = body.chars().count();
    if len == 0 {
        return Err(fail(
            FIELD,
            "must not be empty after removing the leading @",
        ));
    }
    if len > AUTHOR_LOOKUP_MAX_CHARS {
        return Err(fail(
            FIELD,
            "must be 1..=512 characters after removing the leading @",
        ));
    }
    if body.chars().any(|c| c.is_control() || c == '\u{FFFD}') {
        return Err(fail(
            FIELD,
            "must not contain control or invalid characters",
        ));
    }
    Ok(AuthorPrefix(body.to_owned()))
}

/// The form used ONLY to compare a prefix with a display name or an acct:
/// Unicode NFKC normalisation followed by Unicode lowercasing. Both sides are
/// folded, the displayed text of a result is never changed.
pub fn fold_for_match(text: &str) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfkc()
        .normalize(text)
        .to_lowercase()
}

/// Exactly one value of query parameter `name`, form-decoded (`+` is a space,
/// `%XX` is decoded). None or several values are `400 invalid_request`.
fn single_query_value(query: Option<&str>, name: &str) -> Result<String, Vec<FieldViolation>> {
    let mut values = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned());
    match (values.next(), values.next()) {
        (Some(value), None) => Ok(value),
        (None, _) => Err(fail(name, "is required")),
        (Some(_), Some(_)) => Err(fail(name, "must be given exactly once")),
    }
}

/// Validate the raw query string of `resolveNoteSearchAuthor`: exactly one `acct`
/// (other parameters are ignored) that passes [`normalize_acct`].
pub fn parse_resolve_query(query: Option<&str>) -> Result<AuthorAcct, Vec<FieldViolation>> {
    normalize_acct(&single_query_value(query, "acct")?)
}

/// Validate the raw query string of `suggestNoteSearchAuthors`: exactly one
/// `prefix` (other parameters are ignored) that passes [`normalize_prefix`].
pub fn parse_suggestions_query(query: Option<&str>) -> Result<AuthorPrefix, Vec<FieldViolation>> {
    normalize_prefix(&single_query_value(query, "prefix")?)
}

/// Query of `resolveNoteSearchAuthor`. The server resolves only an acct it can
/// confirm locally; it never asks the remote server (no WebFinger, no actor fetch).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ResolveAuthorParams {
    /// Exactly one acct typed after `from:@`: `user@host`, `@user@host`, or without a host
    /// `user` / `@user`, which is THIS server's username (a host equal to this server's own
    /// host means the same). One leading `@` is removed; the username is not lowercased and no
    /// host is guessed; the host is normalised to lowercase ASCII (IDNA) and must be a valid
    /// DNS name without port, IP address or trailing dot. At most 512 characters after
    /// normalisation (513 raw). Empty, repeated, whitespace, control or bidi control
    /// characters, URL syntax (`from:` / `acct:` prefixes, actor URLs) and an invalid host are
    /// `400 invalid_request` with `details.field = acct`. It resolves only through a username
    /// this server has confirmed or a verified remote acct mapping, never through an acct
    /// derived for display (`preferred_username` + actor host, not verified by WebFinger).
    #[param(min_length = 1, max_length = 513)]
    pub acct: String,
}

/// Query of `suggestNoteSearchAuthors`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SuggestAuthorsParams {
    /// Exactly one prefix typed after `from:@`, matched against the display name OR the
    /// acct of candidate authors from the front only: no substring, regular expression or
    /// wildcard, and `%` / `_` are literal characters. One leading `@` is removed; 1..=512
    /// characters remain (513 raw); spaces are allowed, control characters are not
    /// (`400 invalid_request`, `details.field = prefix`). Matching alone applies Unicode
    /// normalisation (NFKC) and lowercasing to both sides; returned text keeps its stored form.
    #[param(min_length = 1, max_length = 513)]
    pub prefix: String,
}

fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    // The key must be present; `null` is a value. (`default` is deliberately absent.)
    Option::<T>::deserialize(deserializer)
}

/// One author of a lookup result. Closed: it carries no inbox, public key, Account
/// ID, relationship state (follow / mute / block), fetch origin or cache time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchAuthor {
    /// Canonical actor ID URL, the same type and constraints as the `author` of
    /// `searchPersonalNotes` (`ActorUrl`): a result can be passed straight to it.
    pub actor_url: ActorUrl,
    /// `user@host` without a leading `@`. An author of this server carries this server's
    /// own host. In a resolve answer it is the confirmed or verified acct; in a suggestion
    /// it is the acct shown for the author, which for a remote author may be derived for
    /// display only and therefore need not resolve. A chosen candidate is identified by
    /// `actor_url`, never by resolving this text again.
    #[schema(
        min_length = 1,
        max_length = 512,
        pattern = r"^[^@\s]+@[^@\s]+$",
        example = "alice@remote.example"
    )]
    pub acct: String,
    /// Display name as stored, or `null` when the author has none. The key is always present.
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true)]
    pub display_name: Option<String>,
    /// Avatar URL (https) as stored, or `null`. The key is always present. A browser that
    /// loads a remote icon directly reveals its IP address to that server: how icons are
    /// fetched or proxied is the frontends' image policy, not part of this contract.
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true, max_length = 2048, pattern = r"^https://[^\s]+$")]
    pub icon_url: Option<String>,
}

/// Answer of `resolveNoteSearchAuthor`: the one author the acct designates.
///
/// `Cache-Control: private, no-store` applies (the answer depends on the viewing Profile).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveAuthorResponse {
    pub profile_id: ProfileNanoid,
    pub author: SearchAuthor,
}

/// Answer of `suggestNoteSearchAuthors`: up to five candidates, best first. No paging,
/// no total, no hidden count. Never contains an author the Profile mutes or blocks
/// (individually or by domain), and no actor URL twice. An empty `items` is an ordinary
/// `200` (nothing matches), never a stand-in for an error.
///
/// `Cache-Control: private, no-store` applies (the answer depends on the viewing Profile).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields, try_from = "SuggestAuthorsResponseWire")]
pub struct SuggestAuthorsResponse {
    pub profile_id: ProfileNanoid,
    #[schema(max_items = 5)]
    pub items: Vec<SearchAuthor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestAuthorsResponseWire {
    profile_id: ProfileNanoid,
    items: Vec<SearchAuthor>,
}

impl TryFrom<SuggestAuthorsResponseWire> for SuggestAuthorsResponse {
    type Error = String;

    fn try_from(wire: SuggestAuthorsResponseWire) -> Result<Self, Self::Error> {
        if wire.items.len() > AUTHOR_SUGGESTION_MAX_ITEMS {
            return Err("at most 5 suggestions are allowed".to_owned());
        }
        for (index, item) in wire.items.iter().enumerate() {
            if wire.items[..index]
                .iter()
                .any(|earlier| earlier.actor_url == item.actor_url)
            {
                return Err("an actor_url must not appear twice".to_owned());
            }
        }
        Ok(Self {
            profile_id: wire.profile_id,
            items: wire.items,
        })
    }
}

/// Resolve one acct to the author it designates (contract only; always 501).
///
/// The target set is the authors ShuttlePub has already fetched notes of AND the viewing
/// Profile may publicly refer to. The acct is resolved only through a username this server
/// has confirmed or a verified remote acct mapping. The server never asks the remote
/// server: no WebFinger, no actor fetch, synchronous or asynchronous. An acct that cannot
/// be resolved is `404 not_found` (`details.field = acct`), never a guess; several matching
/// mappings are `409 conflict`, never the first one. Mute/block does not hide an author
/// here (the acct was typed explicitly, and the later search follows `searchPersonalNotes`:
/// an author of a muted or blocked domain gives an ordinary empty 200). Every response
/// carries `Cache-Control: private, no-store`. Contract only: it never searches. Always 501.
#[utoipa::path(
    get, path = "/api/v1/profiles/{profile_nanoid}/search/authors/resolve",
    operation_id = "resolveNoteSearchAuthor", tag = "note-search-author",
    params(ResolveAuthorParams, ("profile_nanoid" = ProfileNanoid, Path, description = "Viewing Profile (Emumet profile_nanoid)")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub. Cache-Control: private, no-store.", body = ResolveAuthorResponse,
            headers(("Cache-Control" = String, description = "Always `private, no-store`"))),
        (status = 400, description = "invalid_request: acct missing, repeated, empty, over-long, containing whitespace, control or bidi control characters or URL syntax, or with an invalid host (details.field = acct)", body = ErrorBody),
        (status = 401, description = "unauthorized", body = ErrorBody),
        (status = 403, description = "forbidden: the caller cannot act as this Profile", body = ErrorBody),
        (status = 404, description = "not_found: the Profile, or an acct that is not a fetched author this server can confirm (username unknown here, no verified remote acct mapping; details.field = acct). Never produced by asking the remote server.", body = ErrorBody),
        (status = 409, description = "conflict: several verified mappings match the acct, so it cannot be decided (details.field = acct). The first match is never picked silently.", body = ErrorBody),
        (status = 429, description = "rate_limited: too many lookups", body = ErrorBody,
            headers(("Retry-After" = u32, description = "Seconds to wait before retrying"))),
        (status = 500, description = "internal: unexpected failure; never turned into a missing author", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
        (status = 503, description = "author_lookup_timeout or author_lookup_unavailable: the INTERNAL author lookup (not WebFinger) timed out or is unavailable; distinct from search_timeout of searchPersonalNotes. Never turned into a missing author.", body = ErrorBody),
    )
)]
pub async fn resolve_note_search_author() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Author candidates for the text typed after `from:@` (contract only; always 501).
///
/// At most 5 authors whose display name or acct starts with the prefix, among the authors
/// ShuttlePub has already fetched notes of AND the viewing Profile may publicly refer to.
/// Authors the Profile mutes or blocks (individually or by domain) are never candidates.
/// Emumet's shared display cache is not a source (it would leak fetch history caused by
/// other Profiles' block/mute), and no external lookup is made, synchronously or
/// asynchronously. Order: acct equals the prefix, then acct starts with it, then display name
/// starts with it; ties by normalised acct, then actor URL. One entry per actor URL. No
/// candidate is `200` with `items: []`. Every response carries
/// `Cache-Control: private, no-store`. Always 501.
#[utoipa::path(
    get, path = "/api/v1/profiles/{profile_nanoid}/search/authors/suggestions",
    operation_id = "suggestNoteSearchAuthors", tag = "note-search-author",
    params(SuggestAuthorsParams, ("profile_nanoid" = ProfileNanoid, Path, description = "Viewing Profile (Emumet profile_nanoid)")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Future contract response, never returned by the stub. 0..=5 candidates; none is `items: []`. Cache-Control: private, no-store.", body = SuggestAuthorsResponse,
            headers(("Cache-Control" = String, description = "Always `private, no-store`"))),
        (status = 400, description = "invalid_request: prefix missing, repeated, empty (also after removing the leading @), over-long or containing control characters (details.field = prefix)", body = ErrorBody),
        (status = 401, description = "unauthorized", body = ErrorBody),
        (status = 403, description = "forbidden: the caller cannot act as this Profile", body = ErrorBody),
        (status = 404, description = "not_found: the Profile", body = ErrorBody),
        (status = 429, description = "rate_limited: too many lookups", body = ErrorBody,
            headers(("Retry-After" = u32, description = "Seconds to wait before retrying"))),
        (status = 500, description = "internal: unexpected failure; never turned into an empty candidate list", body = ErrorBody),
        (status = 501, description = "Contract only; always returned until implemented", body = ErrorBody),
        (status = 503, description = "author_lookup_timeout or author_lookup_unavailable: the INTERNAL author lookup (not WebFinger) timed out or is unavailable; distinct from search_timeout. Never turned into an empty candidate list.", body = ErrorBody),
    )
)]
pub async fn suggest_note_search_authors() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Both author lookup routes, registered once; no extractors on contract stubs.
pub fn router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(resolve_note_search_author))
        .routes(routes!(suggest_note_search_authors))
}
