//! D04 note search author lookup contract evidence (`resolveNoteSearchAuthor`,
//! `suggestNoteSearchAuthors`). The reference projection below is TEST-ONLY:
//! production routes stay explicit 501 stubs, never search and never ask a remote server.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use jsonschema::Draft;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use server::api_v1::{
    ErrorBody, ErrorCode, FieldViolation,
    personal_domain::{ActorUrl, is_canonical_actor_url},
    quick_reaction::ProfileNanoid,
    search_author::{
        AUTHOR_LOOKUP_MAX_CHARS, AUTHOR_SUGGESTION_MAX_ITEMS, AuthorAcct, ResolveAuthorResponse,
        SearchAuthor, SuggestAuthorsResponse, fold_for_match, normalize_acct, normalize_prefix,
        parse_resolve_query, parse_suggestions_query,
    },
};
use std::{collections::BTreeSet, path::PathBuf};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RESOLVE_PATH: &str = "/api/v1/profiles/{profile_nanoid}/search/authors/resolve";
const SUGGEST_PATH: &str = "/api/v1/profiles/{profile_nanoid}/search/authors/suggestions";
const PROFILE: &str = "V1StGXR8_Z5jdHi6B-myT";

fn fixture(name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/note_search_author")
            .join(name),
    )?)?)
}
fn spec() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&server::openapi::render()?)?)
}
fn valid(spec: &Value, schema: &Value, payload: &Value) -> Result<bool, String> {
    let root = json!({"components": spec["components"], "allOf": [schema]});
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .build(&root)
        .map_err(|e| e.to_string())?;
    Ok(validator.is_valid(payload))
}
fn component(spec: &Value, name: &str) -> Value {
    assert!(
        spec["components"]["schemas"][name].is_object(),
        "missing {name}"
    );
    json!({"$ref": format!("#/components/schemas/{name}")})
}
fn check(spec: &Value, name: &str, payload: &Value) -> TestResult {
    assert!(
        valid(spec, &component(spec, name), payload)?,
        "{name}: {payload}"
    );
    Ok(())
}
fn roundtrip<T: Serialize + DeserializeOwned>(v: &Value) -> TestResult {
    let typed: T = serde_json::from_value(v.clone())?;
    assert_eq!(&serde_json::to_value(typed)?, v);
    Ok(())
}
fn serde_accepts(schema: &str, payload: &Value) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(match schema {
        "SearchAuthor" => serde_json::from_value::<SearchAuthor>(payload.clone()).is_ok(),
        "ResolveAuthorResponse" => {
            serde_json::from_value::<ResolveAuthorResponse>(payload.clone()).is_ok()
        }
        "SuggestAuthorsResponse" => {
            serde_json::from_value::<SuggestAuthorsResponse>(payload.clone()).is_ok()
        }
        "ErrorBody" => serde_json::from_value::<ErrorBody>(payload.clone()).is_ok(),
        other => return Err(format!("no type for {other}").into()),
    })
}

trait Q<T> {
    fn q(self) -> Result<T, Box<dyn std::error::Error>>;
}
impl<T> Q<T> for Result<T, Vec<FieldViolation>> {
    fn q(self) -> Result<T, Box<dyn std::error::Error>> {
        self.map_err(|violations| format!("{violations:?}").into())
    }
}

fn only_violation(
    result: Result<impl std::fmt::Debug, Vec<FieldViolation>>,
    field: &str,
    context: &str,
) {
    match result {
        Ok(value) => panic!("{context}: expected a 400 on {field}, got {value:?}"),
        Err(violations) => {
            assert_eq!(violations.len(), 1, "{context}: {violations:?}");
            assert_eq!(violations[0].field, field, "{context}");
            assert!(!violations[0].message.is_empty(), "{context}");
        }
    }
}

// ---------------------------------------------------------------- pure input checks

#[test]
fn acct_is_normalized_without_guessing_or_lowercasing_the_user() -> TestResult {
    let cases: Vec<(String, &str, Option<&str>)> = vec![
        (
            "alice@remote.example".into(),
            "alice",
            Some("remote.example"),
        ),
        (
            "@alice@remote.example".into(),
            "alice",
            Some("remote.example"),
        ),
        ("alice".into(), "alice", None),
        ("@alice".into(), "alice", None),
        (
            "Alice@Remote.EXAMPLE".into(),
            "Alice",
            Some("remote.example"),
        ),
        ("@Alice".into(), "Alice", None),
        (
            "alice@sub.remote.example".into(),
            "alice",
            Some("sub.remote.example"),
        ),
        ("alice@localhost".into(), "alice", Some("localhost")),
        ("alice@a-b.example".into(), "alice", Some("a-b.example")),
        (
            "alice@xn--wgv71a.example".into(),
            "alice",
            Some("xn--wgv71a.example"),
        ),
        (
            "alice@日本.example".into(),
            "alice",
            Some("xn--wgv71a.example"),
        ),
        (
            "alice@Bücher.example".into(),
            "alice",
            Some("xn--bcher-kva.example"),
        ),
        ("a.b-c_d+e@x.example".into(), "a.b-c_d+e", Some("x.example")),
        (
            "ａｌｉｃｅ@x.example".into(),
            "ａｌｉｃｅ",
            Some("x.example"),
        ),
        ("日本語@x.example".into(), "日本語", Some("x.example")),
    ];
    for (raw, username, host) in cases {
        let parsed = normalize_acct(&raw).map_err(|e| format!("{raw}: {e:?}"))?;
        assert_eq!(
            parsed,
            AuthorAcct {
                username: username.to_owned(),
                host: host.map(str::to_owned)
            },
            "{raw}"
        );
    }
    assert_eq!(
        normalize_acct("@Alice@Remote.Example")
            .q()?
            .to_acct_string(),
        "Alice@remote.example"
    );
    assert_eq!(normalize_acct("@alice").q()?.to_acct_string(), "alice");
    Ok(())
}

#[test]
fn acct_rejections_are_400_on_the_acct_field() {
    let long_label = format!("alice@{}.example", "a".repeat(64));
    let long_host = format!(
        "alice@{}.{}.{}.{}.com",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(63)
    );
    let rejected: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("only at", "@".into()),
        ("two ats", "@@".into()),
        ("double leading at", "@@alice".into()),
        ("double leading at with host", "@@alice@x.example".into()),
        ("missing host", "alice@".into()),
        ("missing host after leading at", "@alice@".into()),
        ("empty user with host", "@@x.example".into()),
        ("three parts", "alice@bob@x.example".into()),
        ("space in user", "alice smith@x.example".into()),
        ("leading space", " alice".into()),
        ("trailing space", "alice ".into()),
        ("tab", "alice\t@x.example".into()),
        ("newline", "alice\n".into()),
        ("ideographic space", "alice\u{3000}@x.example".into()),
        ("nul", "ali\u{0}ce".into()),
        ("del", "ali\u{7f}ce".into()),
        ("c1 control", "ali\u{9f}ce".into()),
        ("rtl override", "al\u{202E}ice@x.example".into()),
        ("ltr mark", "al\u{200E}ice".into()),
        ("rtl mark", "al\u{200F}ice".into()),
        ("isolate in host", "alice@x\u{2066}.example".into()),
        ("arabic letter mark", "al\u{061C}ice".into()),
        ("replacement character", "alice@x.example\u{FFFD}".into()),
        ("from prefix", "from:@alice@x.example".into()),
        ("from prefix without at", "from:alice".into()),
        ("acct scheme prefix", "acct:alice@x.example".into()),
        ("actor url", "https://x.example/users/alice".into()),
        ("scheme relative url", "//x.example/users/alice".into()),
        ("host and path", "x.example/@alice".into()),
        ("path after host", "alice@x.example/path".into()),
        ("query after host", "alice@x.example?q=1".into()),
        ("fragment after host", "alice@x.example#f".into()),
        ("port in host", "alice@x.example:8443".into()),
        ("percent escape", "alice%40x.example".into()),
        ("angle brackets", "<alice>@x.example".into()),
        ("square brackets", "[alice]@x.example".into()),
        ("ipv6 literal", "alice@[::1]".into()),
        ("backslash", "alice\\bob@x.example".into()),
        ("leading hyphen label", "alice@-x.example".into()),
        ("trailing hyphen label", "alice@x-.example".into()),
        ("empty label", "alice@x..example".into()),
        ("leading dot", "alice@.x.example".into()),
        ("trailing dot", "alice@x.example.".into()),
        ("dot only", "alice@.".into()),
        ("ipv4", "alice@1.2.3.4".into()),
        ("ipv4 shorthand", "alice@1.2.3".into()),
        ("ipv4 hex", "alice@0x7f.1".into()),
        ("numeric tld", "alice@x.123".into()),
        ("underscore in host", "alice@x_y.example".into()),
        ("dollar in host", "alice@exa$mple.example".into()),
        ("label over 63 octets", long_label),
        ("host over 253 octets", long_host),
        ("invalid idna", "alice@xn--a.example".into()),
    ];
    for (name, raw) in &rejected {
        only_violation(normalize_acct(raw), "acct", name);
    }
}

#[test]
fn acct_length_boundaries_apply_to_the_normalized_value() -> TestResult {
    let max = AUTHOR_LOOKUP_MAX_CHARS;
    let user_only = "a".repeat(max);
    assert_eq!(normalize_acct(&user_only).q()?.username.len(), max);
    assert_eq!(
        normalize_acct(&format!("@{user_only}")).q()?.username.len(),
        max
    );
    only_violation(normalize_acct(&"a".repeat(max + 1)), "acct", "513 user");
    only_violation(
        normalize_acct(&format!("@{}", "a".repeat(max + 1))),
        "acct",
        "@ + 513 user",
    );
    only_violation(
        normalize_acct(&format!("@@{user_only}")),
        "acct",
        "raw 514 characters",
    );
    // user@host measured as a whole: 500 + '@' + "example.com" (11) = 512.
    let at_limit = format!("{}@example.com", "a".repeat(max - 12));
    assert_eq!(
        normalize_acct(&at_limit)
            .q()?
            .to_acct_string()
            .chars()
            .count(),
        max
    );
    assert!(normalize_acct(&format!("@{at_limit}")).is_ok());
    only_violation(
        normalize_acct(&format!("{}@example.com", "a".repeat(max - 11))),
        "acct",
        "513 normalized",
    );
    // Characters, not bytes.
    assert!(normalize_acct(&"日".repeat(max)).is_ok());
    only_violation(normalize_acct(&"日".repeat(max + 1)), "acct", "513 chars");
    // The host is measured after IDNA: 日本 → xn--wgv71a (10 octets).
    let idn = format!(
        "{}@日本.example",
        "a".repeat(max - 1 - "xn--wgv71a.example".len())
    );
    assert_eq!(
        normalize_acct(&idn).q()?.to_acct_string().chars().count(),
        max
    );
    only_violation(
        normalize_acct(&format!("a{idn}")),
        "acct",
        "idna expands past the limit",
    );
    // Host limits: label 63 ok / 64 not; whole host 253 ok / 254 not.
    let label63 = "a".repeat(63);
    assert!(normalize_acct(&format!("u@{label63}.example")).is_ok());
    let host253 = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(host253.len(), 253);
    assert!(normalize_acct(&format!("u@{host253}")).is_ok());
    only_violation(
        normalize_acct(&format!("u@{host253}d")),
        "acct",
        "254 octet host",
    );
    Ok(())
}

#[test]
fn prefix_removes_one_leading_at_and_keeps_everything_else_literal() -> TestResult {
    let cases = [
        ("ali", "ali"),
        ("@ali", "ali"),
        ("@@ali", "@ali"),
        ("Alice Sm", "Alice Sm"),
        ("alice@remote.ex", "alice@remote.ex"),
        ("日本語", "日本語"),
        ("ＡＬＩＣＥ", "ＡＬＩＣＥ"),
        ("100%_", "100%_"),
        ("%", "%"),
        ("_", "_"),
        ("a\\b", "a\\b"),
        ("ali.*", "ali.*"),
        ("👨\u{200D}👩\u{200D}👧", "👨\u{200D}👩\u{200D}👧"),
        ("Alice\u{200B}", "Alice\u{200B}"),
    ];
    for (raw, expected) in cases {
        assert_eq!(
            normalize_prefix(raw)
                .map_err(|e| format!("{raw}: {e:?}"))?
                .0,
            expected
        );
    }
    let max = AUTHOR_LOOKUP_MAX_CHARS;
    assert!(normalize_prefix(&"a".repeat(max)).is_ok());
    assert!(normalize_prefix(&format!("@{}", "a".repeat(max))).is_ok());
    assert!(normalize_prefix(&"日".repeat(max)).is_ok());
    only_violation(normalize_prefix(&"a".repeat(max + 1)), "prefix", "513");
    only_violation(
        normalize_prefix(&format!("@{}", "a".repeat(max + 1))),
        "prefix",
        "@ + 513",
    );
    only_violation(
        normalize_prefix(&format!("@@{}", "a".repeat(max))),
        "prefix",
        "raw 514",
    );
    for (name, raw) in [
        ("empty", ""),
        ("only at", "@"),
        ("nul", "ali\u{0}"),
        ("newline", "ali\n"),
        ("del", "ali\u{7f}"),
        ("c1 control", "ali\u{85}"),
        ("replacement character", "ali\u{FFFD}"),
    ] {
        only_violation(normalize_prefix(raw), "prefix", name);
    }
    Ok(())
}

#[test]
fn matching_fold_normalizes_unicode_and_lowercases_without_touching_display() {
    assert_eq!(fold_for_match("ＡＬＩＣＥ"), "alice");
    assert_eq!(fold_for_match("Alice"), "alice");
    assert_eq!(fold_for_match("e\u{301}"), fold_for_match("\u{e9}"));
    assert_eq!(fold_for_match("Ｅ\u{301}"), fold_for_match("é"));
    assert_eq!(fold_for_match("ﬁsh"), "fish");
    assert_eq!(fold_for_match("日本語"), "日本語");
    assert_eq!(fold_for_match("100%_"), "100%_");
    assert_eq!(
        fold_for_match("Ａlice@Remote.Example"),
        "alice@remote.example"
    );
    assert_eq!(fold_for_match("ÀB"), "àb");
}

#[test]
fn query_strings_need_exactly_one_value_and_are_form_decoded() -> TestResult {
    assert_eq!(
        parse_resolve_query(Some("acct=%40Alice%40Remote.EXAMPLE"))
            .q()?
            .to_acct_string(),
        "Alice@remote.example"
    );
    assert_eq!(
        parse_resolve_query(Some("acct=alice&other=1&x"))
            .q()?
            .to_acct_string(),
        "alice"
    );
    assert_eq!(
        parse_resolve_query(Some("x=1&acct=%E6%97%A5%E6%9C%AC%40x.example"))
            .q()?
            .username,
        "日本"
    );
    only_violation(parse_resolve_query(None), "acct", "no query");
    only_violation(parse_resolve_query(Some("")), "acct", "empty query");
    only_violation(parse_resolve_query(Some("other=1")), "acct", "absent");
    only_violation(parse_resolve_query(Some("acct=")), "acct", "empty value");
    only_violation(
        parse_resolve_query(Some("acct=a&acct=b")),
        "acct",
        "repeated",
    );
    only_violation(
        parse_resolve_query(Some("acct=a&acct=a")),
        "acct",
        "repeated equal",
    );
    only_violation(
        parse_resolve_query(Some("acct=alice+bob")),
        "acct",
        "plus is a space",
    );
    only_violation(
        parse_resolve_query(Some("acct=alice%20bob")),
        "acct",
        "encoded space",
    );
    only_violation(
        parse_resolve_query(Some("acct=alice%00")),
        "acct",
        "encoded nul",
    );
    only_violation(
        parse_resolve_query(Some("acct=alice%FF")),
        "acct",
        "invalid utf-8",
    );
    only_violation(
        parse_resolve_query(Some("acct=https%3A%2F%2Fx.example%2Fusers%2Falice")),
        "acct",
        "encoded url",
    );
    only_violation(
        parse_resolve_query(Some("acct=from%3A%40alice")),
        "acct",
        "encoded from:",
    );
    only_violation(
        parse_resolve_query(Some("author=https%3A%2F%2Fx.example%2Fusers%2Falice")),
        "acct",
        "wrong key",
    );

    assert_eq!(
        parse_suggestions_query(Some("prefix=Alice+Sm")).q()?.0,
        "Alice Sm"
    );
    assert_eq!(parse_suggestions_query(Some("prefix=%40ali")).q()?.0, "ali");
    assert_eq!(
        parse_suggestions_query(Some("prefix=100%25_")).q()?.0,
        "100%_"
    );
    assert_eq!(
        parse_suggestions_query(Some("prefix=%E6%97%A5%E6%9C%AC"))
            .q()?
            .0,
        "日本"
    );
    assert_eq!(parse_suggestions_query(Some("prefix=a&q=b")).q()?.0, "a");
    only_violation(parse_suggestions_query(None), "prefix", "no query");
    only_violation(
        parse_suggestions_query(Some("acct=alice")),
        "prefix",
        "wrong key",
    );
    only_violation(parse_suggestions_query(Some("prefix=")), "prefix", "empty");
    only_violation(
        parse_suggestions_query(Some("prefix=%40")),
        "prefix",
        "only at",
    );
    only_violation(
        parse_suggestions_query(Some("prefix=a&prefix=b")),
        "prefix",
        "repeated",
    );
    only_violation(
        parse_suggestions_query(Some("prefix=a%0Ab")),
        "prefix",
        "newline",
    );
    only_violation(
        parse_suggestions_query(Some("prefix=%FF")),
        "prefix",
        "invalid utf-8",
    );
    Ok(())
}

// ---------------------------------------------------------------- generated contract

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn param<'a>(op: &'a Value, name: &str) -> Result<&'a Value, Box<dyn std::error::Error>> {
    Ok(op["parameters"]
        .as_array()
        .ok_or("parameters")?
        .iter()
        .find(|p| p["name"] == name)
        .ok_or("parameter")?)
}

fn error_codes_of(op: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(op["responses"]
        .as_object()
        .ok_or("responses")?
        .keys()
        .cloned()
        .collect())
}

#[test]
fn both_operations_are_registered_once_with_the_agreed_contract() -> TestResult {
    let spec = spec()?;
    let lookup_paths: Vec<_> = spec["paths"]
        .as_object()
        .ok_or("paths")?
        .keys()
        .filter(|p| p.contains("/search/authors/"))
        .cloned()
        .collect();
    assert_eq!(lookup_paths, [RESOLVE_PATH, SUGGEST_PATH]);
    for path in [RESOLVE_PATH, SUGGEST_PATH] {
        let item = spec["paths"][path].as_object().ok_or("path item")?;
        assert_eq!(item.keys().collect::<Vec<_>>(), ["get"], "{path}: GET only");
        let op = &item["get"];
        assert_eq!(op["security"], json!([{"bearer_auth": []}]), "{path}");
        assert_eq!(op["tags"], json!(["note-search-author"]), "{path}");
        let profile = param(op, "profile_nanoid")?;
        assert_eq!(profile["in"], "path");
        assert_eq!(profile["required"], true);
        assert_eq!(
            profile["schema"]["$ref"],
            "#/components/schemas/ProfileNanoid"
        );
        for code in error_codes_of(op)? {
            if code == "200" {
                continue;
            }
            assert_eq!(
                op["responses"][&code]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/ErrorBody",
                "{path} {code}"
            );
        }
        assert_eq!(
            op["responses"]["200"]["headers"]["Cache-Control"]["description"],
            "Always `private, no-store`"
        );
        assert!(
            op["description"]
                .as_str()
                .ok_or("description")?
                .contains("`Cache-Control: private, no-store`"),
            "{path}"
        );
        let all_text = serde_json::to_string(op)?;
        for text in ["WebFinger", "ShuttlePub has already fetched"] {
            assert!(all_text.contains(text), "{path} misses {text}");
        }
        let exclusion = if path == SUGGEST_PATH {
            "mutes or blocks"
        } else {
            "Mute/block does not hide an author"
        };
        assert!(all_text.contains(exclusion), "{path} misses {exclusion}");
        assert_eq!(
            op["responses"]["429"]["headers"]["Retry-After"]["schema"]["type"],
            "integer"
        );
        let r503 = op["responses"]["503"]["description"]
            .as_str()
            .ok_or("503")?;
        assert!(
            r503.contains("author_lookup_timeout")
                && r503.contains("author_lookup_unavailable")
                && r503.contains("not WebFinger")
                && r503.contains("search_timeout"),
            "{path}"
        );
    }

    let resolve = &spec["paths"][RESOLVE_PATH]["get"];
    assert_eq!(resolve["operationId"], "resolveNoteSearchAuthor");
    assert_eq!(
        error_codes_of(resolve)?,
        [
            "200", "400", "401", "403", "404", "409", "429", "500", "501", "503"
        ]
    );
    assert_eq!(
        resolve["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ResolveAuthorResponse"
    );
    let acct = param(resolve, "acct")?;
    assert_eq!(acct["in"], "query");
    assert_eq!(acct["required"], true);
    assert_eq!(acct["schema"]["type"], "string");
    assert_eq!(acct["schema"]["minLength"], 1);
    assert_eq!(acct["schema"]["maxLength"], AUTHOR_LOOKUP_MAX_CHARS + 1);
    let acct_doc = flat(acct["description"].as_str().ok_or("acct doc")?);
    for text in [
        "THIS server's username",
        "never through an acct derived for display",
        "verified remote acct mapping",
        "details.field = acct",
        "not lowercased",
        "no host is guessed",
    ] {
        assert!(acct_doc.contains(text), "acct doc misses {text}");
    }
    let r404 = resolve["responses"]["404"]["description"]
        .as_str()
        .ok_or("404")?;
    assert!(
        r404.contains("details.field = acct")
            && r404.contains("Never produced by asking the remote server")
    );
    let r409 = resolve["responses"]["409"]["description"]
        .as_str()
        .ok_or("409")?;
    assert!(r409.contains("never picked silently") && r409.contains("details.field = acct"));

    let suggest = &spec["paths"][SUGGEST_PATH]["get"];
    assert_eq!(suggest["operationId"], "suggestNoteSearchAuthors");
    assert_eq!(
        error_codes_of(suggest)?,
        [
            "200", "400", "401", "403", "404", "429", "500", "501", "503"
        ]
    );
    assert_eq!(
        suggest["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SuggestAuthorsResponse"
    );
    let prefix = param(suggest, "prefix")?;
    assert_eq!(prefix["in"], "query");
    assert_eq!(prefix["required"], true);
    assert_eq!(prefix["schema"]["minLength"], 1);
    assert_eq!(prefix["schema"]["maxLength"], AUTHOR_LOOKUP_MAX_CHARS + 1);
    let prefix_doc = flat(prefix["description"].as_str().ok_or("prefix doc")?);
    for text in [
        "from the front only",
        "regular expression",
        "wildcard",
        "literal",
        "NFKC",
    ] {
        assert!(prefix_doc.contains(text), "prefix doc misses {text}");
    }
    assert!(
        suggest["description"]
            .as_str()
            .ok_or("suggest doc")?
            .contains("acct equals the prefix, then acct starts with it, then display name")
    );
    assert!(
        suggest["responses"]["200"]["description"]
            .as_str()
            .ok_or("200 doc")?
            .contains("none is `items: []`")
    );
    assert!(spec["tags"].as_array().ok_or("tags")?.iter().any(|t| {
        t["name"] == "note-search-author"
            && t["description"]
                .as_str()
                .is_some_and(|d| d.contains("never queries a remote server"))
    }));
    Ok(())
}

#[test]
fn response_schemas_are_closed_and_carry_no_hidden_data() -> TestResult {
    let spec = spec()?;
    let schemas = &spec["components"]["schemas"];
    for name in [
        "SearchAuthor",
        "ResolveAuthorResponse",
        "SuggestAuthorsResponse",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"], false,
            "{name} is closed"
        );
    }
    assert_eq!(
        schemas["SearchAuthor"]["required"],
        json!(["actor_url", "acct", "display_name", "icon_url"])
    );
    let properties = schemas["SearchAuthor"]["properties"]
        .as_object()
        .ok_or("props")?;
    assert_eq!(
        properties.keys().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            &"acct".to_owned(),
            &"actor_url".to_owned(),
            &"display_name".to_owned(),
            &"icon_url".to_owned()
        ])
    );
    // The very same type as the `author` of searchPersonalNotes.
    assert_eq!(
        properties["actor_url"]["$ref"],
        "#/components/schemas/ActorUrl"
    );
    let search = &spec["paths"]["/api/v1/profiles/{profile_nanoid}/notes/search"]["get"];
    assert_eq!(
        param(search, "author")?["schema"]["$ref"],
        "#/components/schemas/ActorUrl"
    );
    assert_eq!(
        properties["display_name"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(properties["icon_url"]["type"], json!(["string", "null"]));
    assert_eq!(
        schemas["SuggestAuthorsResponse"]["properties"]["items"]["maxItems"],
        AUTHOR_SUGGESTION_MAX_ITEMS
    );
    let suggestion_keys: Vec<_> = schemas["SuggestAuthorsResponse"]["properties"]
        .as_object()
        .ok_or("props")?
        .keys()
        .collect();
    assert_eq!(
        suggestion_keys,
        ["items", "profile_id"],
        "no total, no cursor"
    );
    let codes = schemas["ErrorCode"]["enum"].as_array().ok_or("codes")?;
    for code in [
        "rate_limited",
        "author_lookup_timeout",
        "author_lookup_unavailable",
        "search_timeout",
    ] {
        assert!(codes.contains(&json!(code)), "{code}");
    }
    Ok(())
}

#[test]
fn new_error_codes_have_their_own_statuses_and_wire_names() -> TestResult {
    assert_eq!(
        ErrorCode::RateLimited.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        ErrorCode::AuthorLookupTimeout.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        ErrorCode::AuthorLookupUnavailable.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::RateLimited)?,
        "rate_limited"
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::AuthorLookupTimeout)?,
        "author_lookup_timeout"
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::AuthorLookupUnavailable)?,
        "author_lookup_unavailable"
    );
    // Different codes: the author lookup never reuses the note search time limit.
    assert_ne!(ErrorCode::AuthorLookupTimeout, ErrorCode::SearchTimeout);
    assert_ne!(ErrorCode::AuthorLookupUnavailable, ErrorCode::SearchTimeout);
    // Existing meanings are unchanged.
    assert_eq!(
        ErrorCode::SearchTimeout.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(ErrorCode::Conflict.status(), StatusCode::CONFLICT);
    assert_eq!(ErrorCode::NotFound.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        ErrorCode::NotImplemented.status(),
        StatusCode::NOT_IMPLEMENTED
    );
    Ok(())
}

#[test]
fn schema_fixtures_validate_and_serde_agrees() -> TestResult {
    let spec = spec()?;
    let cases = fixture("schema_cases.json")?;
    let mut names = BTreeSet::new();
    let (mut positives, mut negatives) = (0, 0);
    for case in cases.as_array().ok_or("cases")? {
        assert!(
            names.insert(case["name"].as_str().ok_or("name")?),
            "duplicate case name"
        );
        let schema = case["schema"].as_str().ok_or("schema")?;
        let expected = case["valid"].as_bool().ok_or("valid flag")?;
        assert_eq!(
            valid(&spec, &component(&spec, schema), &case["payload"])?,
            expected,
            "{}",
            case["name"]
        );
        if expected {
            positives += 1;
            if case["serde_rejects"].as_bool() == Some(true) {
                // Schema-valid, yet the type refuses it (for example duplicate actor_url).
                assert!(
                    !serde_accepts(schema, &case["payload"])?,
                    "{}",
                    case["name"]
                );
            } else {
                match schema {
                    "SearchAuthor" => roundtrip::<SearchAuthor>(&case["payload"])?,
                    "ResolveAuthorResponse" => {
                        roundtrip::<ResolveAuthorResponse>(&case["payload"])?
                    }
                    "SuggestAuthorsResponse" => {
                        roundtrip::<SuggestAuthorsResponse>(&case["payload"])?
                    }
                    "ErrorBody" => roundtrip::<ErrorBody>(&case["payload"])?,
                    other => return Err(format!("unexpected schema {other}").into()),
                }
            }
        } else {
            negatives += 1;
            let serde_rejects = case["serde_rejects"].as_bool().ok_or("serde_rejects")?;
            assert_eq!(
                !serde_accepts(schema, &case["payload"])?,
                serde_rejects,
                "{}",
                case["name"]
            );
        }
    }
    assert!(
        positives >= 20 && negatives >= 30,
        "{positives}/{negatives}"
    );
    // Every schema and every error the contract names is covered by a fixture.
    for needed in [
        "suggestions_zero",
        "suggestions_one",
        "suggestions_five",
        "suggestions_six",
        "author_display_fields_null",
        "author_local_server",
        "author_actor_url_http",
        "author_display_name_missing",
        "author_unknown_field_inbox",
        "error_404_acct",
        "error_409_acct_ambiguous",
        "error_503_author_lookup_timeout",
        "error_503_author_lookup_unavailable",
        "error_429_rate_limited",
        "error_501_not_implemented",
    ] {
        assert!(names.contains(needed), "fixture {needed} is missing");
    }
    Ok(())
}

#[test]
fn author_acct_pattern_agrees_with_the_input_check_on_what_it_returns() -> TestResult {
    // A returned acct `user@host` is something the resolver input would accept again
    // (host already normalized), so the two schemas never contradict each other.
    let spec = spec()?;
    for acct in [
        "alice@local.example",
        "alice@remote.example",
        "a_c@p.example",
        "日本語@xn--wgv71a.example",
    ] {
        let payload = json!({
            "actor_url": "https://remote.example/users/x",
            "acct": acct,
            "display_name": null,
            "icon_url": null
        });
        check(&spec, "SearchAuthor", &payload)?;
        assert!(normalize_acct(acct).is_ok(), "{acct}");
    }
    Ok(())
}

// ---------------------------------------------------------------- reference projection

#[derive(Clone, Debug, Deserialize)]
struct AuthorRec {
    key: String,
    actor_url: String,
    acct: String,
    display_name: Option<String>,
    icon_url: Option<String>,
    fetched: bool,
    publicly_referable: bool,
    local_username: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Viewer {
    muted_domains: Vec<String>,
    blocked_domains: Vec<String>,
    muted_actors: Vec<String>,
    blocked_actors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Mapping {
    acct: String,
    actor_url: String,
}

#[derive(Debug, PartialEq)]
struct Failure {
    code: ErrorCode,
    field: String,
}

struct World {
    local_host: String,
    authors: Vec<AuthorRec>,
    mappings: Vec<Mapping>,
    viewers: std::collections::BTreeMap<String, Viewer>,
}

fn host_of(actor_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(url::Url::parse(actor_url)?
        .host_str()
        .ok_or("actor url without host")?
        .to_owned())
}

fn load_world() -> Result<(World, Value), Box<dyn std::error::Error>> {
    let doc = fixture("projection.json")?;
    let world = World {
        local_host: doc["local_host"].as_str().ok_or("local_host")?.to_owned(),
        authors: serde_json::from_value(doc["authors"].clone())?,
        mappings: serde_json::from_value(doc["verified_acct_mappings"].clone())?,
        viewers: serde_json::from_value(doc["viewers"].clone())?,
    };
    Ok((world, doc))
}

fn to_search_author(record: &AuthorRec) -> SearchAuthor {
    SearchAuthor {
        actor_url: ActorUrl(record.actor_url.clone()),
        acct: record.acct.clone(),
        display_name: record.display_name.clone(),
        icon_url: record.icon_url.clone(),
    }
}

impl World {
    /// Fetched by ShuttlePub and publicly referable by the viewing Profile: the only
    /// authors either operation may ever show. Nothing here reaches outside this data.
    fn in_target_set(&self) -> impl Iterator<Item = &AuthorRec> {
        self.authors
            .iter()
            .filter(|a| a.fetched && a.publicly_referable)
    }

    fn excluded_for(
        &self,
        viewer: &Viewer,
        author: &AuthorRec,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let host = host_of(&author.actor_url)?;
        Ok(viewer.muted_actors.contains(&author.actor_url)
            || viewer.blocked_actors.contains(&author.actor_url)
            || viewer.muted_domains.contains(&host)
            || viewer.blocked_domains.contains(&host))
    }

    fn suggest(
        &self,
        viewer: &Viewer,
        raw_prefix: &str,
    ) -> Result<Result<Vec<SearchAuthor>, Vec<FieldViolation>>, Box<dyn std::error::Error>> {
        let prefix = match normalize_prefix(raw_prefix) {
            Ok(prefix) => prefix,
            Err(violations) => return Ok(Err(violations)),
        };
        let needle = fold_for_match(&prefix.0);
        let mut ranked: Vec<(u8, String, &AuthorRec)> = Vec::new();
        for author in self.in_target_set() {
            if self.excluded_for(viewer, author)? {
                continue;
            }
            let acct = fold_for_match(&author.acct);
            let name = author.display_name.as_deref().map(fold_for_match);
            let rank = if acct == needle {
                0
            } else if acct.starts_with(&needle) {
                1
            } else if name.is_some_and(|n| n.starts_with(&needle)) {
                2
            } else {
                continue;
            };
            ranked.push((rank, acct, author));
        }
        ranked.sort_by(|x, y| (x.0, &x.1, &x.2.actor_url).cmp(&(y.0, &y.1, &y.2.actor_url)));
        let mut seen = BTreeSet::new();
        Ok(Ok(ranked
            .into_iter()
            .filter(|(_, _, a)| seen.insert(a.actor_url.clone()))
            .take(AUTHOR_SUGGESTION_MAX_ITEMS)
            .map(|(_, _, a)| to_search_author(a))
            .collect()))
    }

    /// Resolution needs a confirmed local username or a verified remote acct mapping.
    /// Mute/block of the viewer is deliberately NOT applied (explicit acct).
    fn resolve(&self, raw_acct: &str) -> Result<SearchAuthor, Failure> {
        let acct = normalize_acct(raw_acct).map_err(|v| Failure {
            code: ErrorCode::InvalidRequest,
            field: v[0].field.clone(),
        })?;
        let candidates: BTreeSet<&str> = match &acct.host {
            None => self.local_candidates(&acct),
            Some(host) if *host == self.local_host => self.local_candidates(&acct),
            Some(host) => {
                let wanted = format!("{}@{host}", acct.username);
                let mapped: BTreeSet<&str> = self
                    .mappings
                    .iter()
                    .filter(|m| m.acct == wanted)
                    .map(|m| m.actor_url.as_str())
                    .collect();
                self.in_target_set()
                    .filter(|a| mapped.contains(a.actor_url.as_str()))
                    .map(|a| a.actor_url.as_str())
                    .collect()
            }
        };
        let mut found = candidates.into_iter();
        let (first, second) = (found.next(), found.next());
        let failure = |code| Failure {
            code,
            field: "acct".to_owned(),
        };
        match (first, second) {
            (None, _) => Err(failure(ErrorCode::NotFound)),
            (Some(_), Some(_)) => Err(failure(ErrorCode::Conflict)),
            (Some(url), None) => self
                .in_target_set()
                .find(|a| a.actor_url == url)
                .map(to_search_author)
                .ok_or_else(|| failure(ErrorCode::NotFound)),
        }
    }

    fn local_candidates(&self, acct: &AuthorAcct) -> BTreeSet<&str> {
        self.in_target_set()
            .filter(|a| a.local_username.as_deref() == Some(acct.username.as_str()))
            .map(|a| a.actor_url.as_str())
            .collect()
    }
}

fn failure_body(failure: &Failure) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::to_value(ErrorBody {
        code: failure.code,
        message: "reference projection".to_owned(),
        details: vec![FieldViolation {
            field: failure.field.clone(),
            message: "reference projection".to_owned(),
        }],
        domain_block: None,
    })?)
}

#[test]
fn reference_projection_satisfies_every_suggestion_case() -> TestResult {
    let (world, doc) = load_world()?;
    let spec = spec()?;
    let cases = doc["suggestion_cases"]
        .as_array()
        .ok_or("suggestion_cases")?;
    assert!(cases.len() >= 30);
    let mut names = BTreeSet::new();
    for case in cases {
        assert!(names.insert(case["name"].as_str().ok_or("name")?));
        let viewer = world
            .viewers
            .get(case["viewer"].as_str().ok_or("viewer")?)
            .ok_or("unknown viewer")?;
        let items = world
            .suggest(viewer, case["prefix"].as_str().ok_or("prefix")?)?
            .map_err(|v| format!("{}: {v:?}", case["name"]))?;
        let urls: Vec<_> = items.iter().map(|a| a.actor_url.0.clone()).collect();
        let expected: Vec<String> = serde_json::from_value(case["expect"].clone())?;
        assert_eq!(urls, expected, "{}", case["name"]);
        assert!(items.len() <= AUTHOR_SUGGESTION_MAX_ITEMS);
        let response = SuggestAuthorsResponse {
            profile_id: ProfileNanoid(PROFILE.to_owned()),
            items: items.clone(),
        };
        let json_response = serde_json::to_value(&response)?;
        check(&spec, "SuggestAuthorsResponse", &json_response)?;
        roundtrip::<SuggestAuthorsResponse>(&json_response)?;
        for item in &items {
            // Every returned actor_url is acceptable as the `author` of searchPersonalNotes (#23).
            assert!(
                is_canonical_actor_url(&item.actor_url.0),
                "{}",
                item.actor_url.0
            );
        }
        // Nothing the viewer mutes or blocks is ever a candidate, and no actor twice.
        for author in &world.authors {
            if world.excluded_for(viewer, author)? {
                assert!(
                    !urls.contains(&author.actor_url),
                    "{}: excluded author {} was returned",
                    case["name"],
                    author.key
                );
            }
            if !author.fetched || !author.publicly_referable {
                assert!(!urls.contains(&author.actor_url), "{}", case["name"]);
            }
        }
        assert_eq!(urls.iter().collect::<BTreeSet<_>>().len(), urls.len());
    }
    // The fixture covers the whole behavior list of the contract.
    for needed in [
        "acct_prefix_then_name_prefix_cut_at_five",
        "muted_domain_never_a_candidate",
        "blocked_individual_never_a_candidate",
        "blocked_domain_never_a_candidate",
        "muted_individual_never_a_candidate",
        "unfetched_author_is_not_a_candidate",
        "not_publicly_referable_author_is_not_a_candidate",
        "acct_exact_match_ranks_before_a_name_prefix_with_an_earlier_acct",
        "fullwidth_prefix_matches_after_normalization_and_lowercasing",
        "percent_underscore_is_literal_prefix",
        "at_most_five_of_seven",
        "ties_order_by_actor_url",
        "duplicate_actor_url_is_listed_once",
        "substring_is_not_a_match",
        "no_match_is_an_empty_200",
    ] {
        assert!(
            names.contains(needed),
            "suggestion case {needed} is missing"
        );
    }
    Ok(())
}

#[test]
fn reference_projection_keeps_display_text_as_stored_while_matching_folded() -> TestResult {
    let (world, _) = load_world()?;
    let viewer = world.viewers.get("viewer_a").ok_or("viewer_a")?;
    let items = world
        .suggest(viewer, "alice")?
        .map_err(|v| format!("{v:?}"))?;
    let wide = items
        .iter()
        .find(|a| a.actor_url.0 == "https://wide.example/users/w")
        .ok_or("wide author")?;
    assert_eq!(wide.display_name.as_deref(), Some("ＡＬＩＣＥ Wide"));
    let first = items.first().ok_or("first")?;
    assert_eq!(
        first.icon_url.as_deref(),
        Some("https://local.example/icons/alice.png")
    );
    Ok(())
}

#[test]
fn reference_projection_satisfies_every_resolve_case() -> TestResult {
    let (world, doc) = load_world()?;
    let spec = spec()?;
    let cases = doc["resolve_cases"].as_array().ok_or("resolve_cases")?;
    assert!(cases.len() >= 15);
    let mut names = BTreeSet::new();
    for case in cases {
        assert!(names.insert(case["name"].as_str().ok_or("name")?));
        let outcome = world.resolve(case["acct"].as_str().ok_or("acct")?);
        match (&outcome, &case["expect"]) {
            (Ok(author), expect) if expect["ok"].is_string() => {
                assert_eq!(
                    Some(author.actor_url.0.as_str()),
                    expect["ok"].as_str(),
                    "{}",
                    case["name"]
                );
                let response = ResolveAuthorResponse {
                    profile_id: ProfileNanoid(PROFILE.to_owned()),
                    author: author.clone(),
                };
                let json_response = serde_json::to_value(&response)?;
                check(&spec, "ResolveAuthorResponse", &json_response)?;
                roundtrip::<ResolveAuthorResponse>(&json_response)?;
                assert!(is_canonical_actor_url(&author.actor_url.0));
            }
            (Err(failure), expect) if expect["err"].is_object() => {
                let expected_code: ErrorCode =
                    serde_json::from_value(expect["err"]["code"].clone())?;
                assert_eq!(failure.code, expected_code, "{}", case["name"]);
                assert_eq!(
                    failure.field,
                    expect["err"]["field"].as_str().ok_or("field")?
                );
                let body = failure_body(failure)?;
                check(&spec, "ErrorBody", &body)?;
                let status = failure.code.status();
                assert!(
                    status == StatusCode::NOT_FOUND || status == StatusCode::CONFLICT,
                    "{}",
                    case["name"]
                );
            }
            other => return Err(format!("{}: unexpected {other:?}", case["name"]).into()),
        }
    }
    // An invalid acct never reaches a lookup: it is a 400 on `acct`.
    let invalid = world.resolve("from:@alice@remote.example");
    assert_eq!(
        invalid,
        Err(Failure {
            code: ErrorCode::InvalidRequest,
            field: "acct".to_owned()
        })
    );
    for needed in [
        "local_username_without_host",
        "verified_remote_acct",
        "display_only_derived_acct_never_resolves",
        "two_verified_mappings_are_a_conflict_not_the_first",
        "username_is_never_lowercased",
        "muted_domain_still_resolves_an_explicit_acct",
    ] {
        assert!(names.contains(needed), "resolve case {needed} is missing");
    }
    Ok(())
}

#[test]
fn a_successful_actor_url_is_accepted_as_the_search_author() -> TestResult {
    let (world, _) = load_world()?;
    for author in world.in_target_set() {
        assert!(
            is_canonical_actor_url(&author.actor_url),
            "{} ({})",
            author.actor_url,
            author.key
        );
        let query = format!(
            "author={}",
            url::form_urlencoded::byte_serialize(author.actor_url.as_bytes()).collect::<String>()
        );
        assert!(query.starts_with("author=https%3A%2F%2F"));
    }
    // The checker is the one of #23: it still refuses what is not an actor ID URL.
    assert!(!is_canonical_actor_url("@alice@remote.example"));
    assert!(!is_canonical_actor_url("alice@remote.example"));
    Ok(())
}

// ---------------------------------------------------------------- stubs and isolation

async fn get(
    app: &axum::Router,
    uri: &str,
    auth: Option<&str>,
) -> Result<(StatusCode, Value, bool), Box<dyn std::error::Error>> {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(auth) = auth {
        builder = builder.header("authorization", auth);
    }
    let response = app.clone().oneshot(builder.body(Body::empty())?).await?;
    let status = response.status();
    let no_cache_header = response.headers().contains_key("cache-control");
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes)?, no_cache_header))
}

#[tokio::test]
async fn both_routes_answer_an_explicit_501_for_any_input_and_any_credentials() -> TestResult {
    let (app, _) = server::openapi::contract();
    let queries = [
        (
            "resolve",
            "acct",
            vec![
                "",
                "?acct=alice",
                "?acct=%40alice%40remote.example",
                "?acct=alice%40remote.example",
                "?acct=",
                "?acct=a&acct=b",
                "?acct=from%3A%40alice",
                "?acct=alice%20bob",
                "?acct=https%3A%2F%2Fremote.example%2Fusers%2Falice",
                "?acct=%FF",
                "?prefix=alice",
                "?acct=alice@1.2.3.4",
                "?acct=%E2%80%AEalice",
            ],
        ),
        (
            "suggestions",
            "prefix",
            vec![
                "",
                "?prefix=ali",
                "?prefix=%40",
                "?prefix=",
                "?prefix=a&prefix=b",
                "?prefix=%00",
                "?prefix=100%25_",
                "?acct=alice",
                "?prefix=%E6%97%A5%E6%9C%AC",
            ],
        ),
    ];
    for (route, _, list) in queries {
        for query in list {
            for auth in [
                None,
                Some("Bearer not-a-real-token"),
                Some("Basic Zm9vOmJhcg=="),
            ] {
                let uri = format!("/api/v1/profiles/{PROFILE}/search/authors/{route}{query}");
                let (status, body, _) = get(&app, &uri, auth).await?;
                assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{uri} {auth:?}");
                let parsed: ErrorBody = serde_json::from_value(body)?;
                assert_eq!(parsed.code, ErrorCode::NotImplemented);
                assert!(parsed.details.is_empty());
            }
        }
    }
    // Even a path parameter that is not a valid Profile nanoid reaches the stub
    // (no extractor, so no extractor rejection either).
    let (status, _, _) = get(
        &app,
        "/api/v1/profiles/not%20a%20nanoid!/search/authors/resolve?acct=x",
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    Ok(())
}

#[tokio::test]
async fn other_methods_and_unknown_siblings_are_not_routes() -> TestResult {
    let (app, _) = server::openapi::contract();
    for (method, uri) in [
        (
            "POST",
            format!("/api/v1/profiles/{PROFILE}/search/authors/resolve"),
        ),
        (
            "PUT",
            format!("/api/v1/profiles/{PROFILE}/search/authors/suggestions"),
        ),
        ("GET", format!("/api/v1/profiles/{PROFILE}/search/authors")),
        (
            "GET",
            format!("/api/v1/profiles/{PROFILE}/search/authors/other"),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert!(
            response.status() == StatusCode::METHOD_NOT_ALLOWED
                || response.status() == StatusCode::NOT_FOUND,
            "{method} {uri}: {}",
            response.status()
        );
    }
    Ok(())
}

#[test]
fn generation_stays_deterministic_and_leaves_existing_search_untouched() -> TestResult {
    let first = server::openapi::render()?;
    assert_eq!(first, server::openapi::render()?);
    let spec: Value = serde_json::from_str(&first)?;
    let search = &spec["paths"]["/api/v1/profiles/{profile_nanoid}/notes/search"]["get"];
    assert_eq!(search["operationId"], "searchPersonalNotes");
    let names: Vec<_> = search["parameters"]
        .as_array()
        .ok_or("parameters")?
        .iter()
        .map(|p| p["name"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(names, ["author", "q", "limit", "cursor", "profile_nanoid"]);
    assert_eq!(
        error_codes_of(search)?,
        ["200", "400", "401", "403", "404", "501", "503"]
    );
    Ok(())
}

#[test]
fn the_module_adds_no_database_http_background_or_auth_dependency() -> TestResult {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))?;
    let mut in_dependencies = false;
    let mut dependencies = BTreeSet::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_dependencies = line == "[dependencies]";
            continue;
        }
        if in_dependencies && let Some((name, _)) = line.split_once('=') {
            dependencies.insert(name.trim().split('.').next().unwrap_or_default().to_owned());
        }
    }
    let expected: BTreeSet<String> = [
        "kernel",
        "application",
        "driver",
        "tokio",
        "serde",
        "serde_json",
        "uuid",
        "url",
        "icu_normalizer",
        "axum",
        "utoipa",
        "utoipa-axum",
        "http-body-util",
        "tracing",
        "tracing-subscriber",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        dependencies, expected,
        "the author lookup contract may only add the pure normalization crate"
    );
    let source = std::fs::read_to_string(root.join("src/api_v1/search_author.rs"))?;
    for forbidden in [
        "driver::",
        "application::",
        "kernel::",
        "reqwest",
        "hyper",
        "tokio::",
        "sqlx",
        "spawn",
        "TcpStream",
        "std::net",
    ] {
        assert!(
            !source.contains(forbidden),
            "search_author.rs uses {forbidden}"
        );
    }
    // The only handlers are extractor-free stubs.
    assert_eq!(source.matches("pub async fn ").count(), 2);
    assert_eq!(source.matches("ApiV1Error::not_implemented()").count(), 2);
    assert!(source.contains("pub async fn resolve_note_search_author() -> ApiV1Error"));
    assert!(source.contains("pub async fn suggest_note_search_authors() -> ApiV1Error"));
    Ok(())
}
