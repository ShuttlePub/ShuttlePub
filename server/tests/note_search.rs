//! D04 note search contract evidence. The reference projection below is
//! TEST-ONLY: production routes stay explicit 501 stubs and never search.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use jsonschema::Draft;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use server::api_v1::{
    ErrorBody, ErrorCode, FieldViolation,
    personal_domain::{
        NOTE_SEARCH_DEFAULT_LIMIT, NOTE_SEARCH_MAX_LIMIT, NOTE_SEARCH_MAX_QUERY_CHARS,
        NoteSearchParams, NoteSearchResponse,
    },
};
use std::{collections::BTreeSet, path::PathBuf};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SEARCH_PATH: &str = "/api/v1/profiles/{profile_nanoid}/notes/search";

fn fixture(name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/personal_domain")
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

fn error_body(
    code: ErrorCode,
    message: &str,
    field: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::to_value(ErrorBody {
        code,
        message: message.to_owned(),
        details: field
            .map(|f| FieldViolation {
                field: f.to_owned(),
                message: message.to_owned(),
            })
            .into_iter()
            .collect(),
        domain_block: None,
    })?)
}

#[test]
fn search_schema_fixtures_validate_and_serde_agrees_on_structural_rules() -> TestResult {
    let spec = spec()?;
    let cases = fixture("search_schema_cases.json")?;
    let mut names = BTreeSet::new();
    for case in cases.as_array().ok_or("cases")? {
        assert!(names.insert(case["name"].as_str().ok_or("name")?));
        let schema = case["schema"].as_str().ok_or("schema")?;
        let expected = case["valid"].as_bool().ok_or("valid flag")?;
        assert_eq!(
            valid(&spec, &component(&spec, schema), &case["payload"])?,
            expected,
            "{}",
            case["name"]
        );
        match (schema, expected) {
            ("NoteSearchResponse", true) => roundtrip::<NoteSearchResponse>(&case["payload"])?,
            ("ErrorBody", true) => roundtrip::<ErrorBody>(&case["payload"])?,
            ("NoteSearchResponse", false) => {
                let serde_rejects = case["serde_rejects"].as_bool().ok_or("serde_rejects")?;
                assert_eq!(
                    serde_json::from_value::<NoteSearchResponse>(case["payload"].clone()).is_err(),
                    serde_rejects,
                    "{}",
                    case["name"]
                );
            }
            _ => {}
        }
    }
    assert!(names.len() >= 30);
    Ok(())
}

#[test]
fn search_operation_declares_the_input_limits_and_error_surface() -> TestResult {
    let spec = spec()?;
    let op = &spec["paths"][SEARCH_PATH]["get"];
    assert_eq!(op["operationId"], "searchPersonalNotes");
    let params = op["parameters"].as_array().ok_or("parameters")?;
    let by_name = |name: &str| params.iter().find(|p| p["name"] == name).cloned();
    let names: Vec<_> = params
        .iter()
        .map(|p| p["name"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(names, ["author", "q", "limit", "cursor", "profile_nanoid"]);
    for name in ["author", "q", "limit", "cursor"] {
        let p = by_name(name).ok_or("param")?;
        assert_eq!(p["in"], "query");
        assert_eq!(p["required"], false, "{name} is optional");
    }
    let author = by_name("author").ok_or("author")?;
    assert_eq!(author["schema"]["$ref"], "#/components/schemas/ActorUrl");
    let author_doc = author["description"].as_str().ok_or("author doc")?;
    assert!(author_doc.contains("never an acct"));
    assert!(author_doc.contains("404"));
    assert!(
        spec["components"]["schemas"]["ActorUrl"]["description"]
            .as_str()
            .ok_or("actor url doc")?
            .contains("from:@user")
    );
    let q = by_name("q").ok_or("q")?;
    assert_eq!(q["schema"]["minLength"], 1);
    assert_eq!(q["schema"]["maxLength"], NOTE_SEARCH_MAX_QUERY_CHARS);
    let q_doc = q["description"].as_str().ok_or("q doc")?;
    for text in [
        "body AND its content warning",
        "UNDECIDED",
        "case-insensitive",
        "ANDed",
    ] {
        assert!(q_doc.contains(text), "q doc misses {text}");
    }
    let limit = by_name("limit").ok_or("limit")?;
    assert_eq!(limit["schema"]["minimum"], 1);
    assert_eq!(limit["schema"]["maximum"], NOTE_SEARCH_MAX_LIMIT);
    assert_eq!(limit["schema"]["default"], NOTE_SEARCH_DEFAULT_LIMIT);
    assert!(
        by_name("cursor").ok_or("cursor")?["description"]
            .as_str()
            .ok_or("cursor doc")?
            .contains("bound to the Profile")
    );
    let responses = op["responses"].as_object().ok_or("responses")?;
    let codes: Vec<_> = responses.keys().map(String::as_str).collect();
    assert_eq!(codes, ["200", "400", "401", "403", "404", "501", "503"]);
    for code in &codes[1..] {
        assert_eq!(
            responses[*code]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ErrorBody",
            "{code}"
        );
    }
    assert_eq!(
        responses["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/NoteSearchResponse"
    );
    let timeout_doc = responses["503"]["description"].as_str().ok_or("503 doc")?;
    assert!(
        timeout_doc.contains("search_timeout")
            && timeout_doc.contains("page.truncated = true")
            && timeout_doc.contains("before any resumable progress")
    );
    assert!(
        spec["components"]["schemas"]["ErrorCode"]["enum"]
            .as_array()
            .ok_or("codes")?
            .contains(&json!("search_timeout"))
    );
    assert_eq!(
        ErrorCode::SearchTimeout.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::SearchTimeout)?,
        json!("search_timeout")
    );
    Ok(())
}

#[test]
fn result_type_is_new_closed_and_cannot_express_a_pure_repost() -> TestResult {
    let spec = spec()?;
    let schemas = &spec["components"]["schemas"];
    assert_eq!(
        schemas["NoteSearchResponse"]["properties"]["items"]["items"]["$ref"],
        "#/components/schemas/NoteSearchItem"
    );
    let mut property_names: Vec<_> = schemas["NoteSearchResponse"]["properties"]
        .as_object()
        .ok_or("properties")?
        .keys()
        .cloned()
        .collect();
    property_names.sort();
    assert_eq!(property_names, ["items", "page", "profile_id"]);
    let kinds: Vec<_> = schemas["NoteSearchItem"]["oneOf"]
        .as_array()
        .ok_or("variants")?
        .iter()
        .map(|v| v["properties"]["kind"]["enum"][0].clone())
        .collect();
    assert_eq!(kinds, [json!("post"), json!("reply"), json!("quote")]);
    for name in ["NoteSearchItem", "SearchNoteBody"] {
        for variant in schemas[name]["oneOf"].as_array().ok_or("variants")? {
            assert_eq!(variant["additionalProperties"], false, "{name}");
        }
    }
    for name in [
        "NoteSearchResponse",
        "NoteSearchItem",
        "SearchedNote",
        "SearchNoteBody",
    ] {
        let encoded = serde_json::to_string(&without_descriptions(&schemas[name]))?;
        assert!(!encoded.contains("repost"), "{name} can express a repost");
        assert!(!encoded.contains("FeedItem"), "{name} reuses FeedItem");
    }
    let cw_variant = &schemas["SearchNoteBody"]["oneOf"][1];
    assert!(cw_variant["properties"].get("content").is_none());
    assert_eq!(
        cw_variant["required"],
        json!(["content_warning", "display"])
    );
    Ok(())
}

fn without_descriptions(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "description")
                .map(|(key, inner)| (key.clone(), without_descriptions(inner)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_descriptions).collect()),
        other => other.clone(),
    }
}

fn sample_params() -> Vec<Value> {
    let long_author = format!("https://remote.example/{}", "a".repeat(2040));
    vec![
        json!({}),
        json!({"limit": 5}),
        json!({"cursor": "opaque"}),
        json!({"q": "hello"}),
        json!({"q": ""}),
        json!({"q": "a".repeat(256)}),
        json!({"q": "a".repeat(257)}),
        json!({"q": "日本語".repeat(86)}),
        json!({"author": "https://remote.example/users/alice"}),
        json!({"author": "https://remote.example/users/alice", "q": "x", "limit": 1}),
        json!({"author": "https://remote.example/users/alice", "limit": 50}),
        json!({"author": "https://remote.example/users/alice", "limit": 0}),
        json!({"author": "https://remote.example/users/alice", "limit": 51}),
        json!({"author": "@alice@remote.example"}),
        json!({"author": "from:@alice", "q": "x"}),
        json!({"author": "http://remote.example/users/alice"}),
        json!({"author": "https:///users/alice"}),
        json!({"author": "https://remote.example/users/alice#main-key"}),
        json!({"author": "https://remote.example/users/a lice"}),
        json!({"author": long_author}),
        json!({"author": format!("{long_author}a")}),
    ]
}

#[test]
fn input_validation_matches_the_generated_parameter_schemas() -> TestResult {
    let spec = spec()?;
    let params = spec["paths"][SEARCH_PATH]["get"]["parameters"]
        .as_array()
        .ok_or("parameters")?;
    let schema_of = |name: &str| {
        params
            .iter()
            .find(|p| p["name"] == name)
            .map(|p| p["schema"].clone())
    };
    let mut samples = sample_params();
    for case in fixture("search.json")?["cases"].as_array().ok_or("cases")? {
        let mut query = case["query"].clone();
        if let Some(repeat) = query.get("q_repeat").cloned() {
            query.as_object_mut().ok_or("query")?.remove("q_repeat");
            query["q"] = json!(expand(&repeat)?);
        }
        samples.push(query);
    }
    for sample in samples {
        let parsed: NoteSearchParams = serde_json::from_value(sample.clone())?;
        let mut schema_ok = parsed.author.is_some() || parsed.q.is_some();
        for name in ["author", "q", "limit", "cursor"] {
            if let (Some(value), Some(schema)) = (sample.get(name), schema_of(name)) {
                schema_ok &= valid(&spec, &schema, value)?;
            }
        }
        let result = parsed.validate();
        assert_eq!(result.is_ok(), schema_ok, "{sample}");
        if let Err(problems) = result {
            assert!(
                problems
                    .iter()
                    .all(|p| ["author", "q", "limit"].contains(&p.field.as_str()))
            );
        }
    }
    Ok(())
}

fn expand(repeat: &Value) -> Result<String, Box<dyn std::error::Error>> {
    Ok(repeat["ch"]
        .as_str()
        .ok_or("ch")?
        .repeat(repeat["count"].as_u64().ok_or("count")? as usize))
}

struct World {
    notes: Vec<Value>,
    policies: Vec<(String, String)>,
    known: BTreeSet<String>,
    viewer: String,
}

impl World {
    fn load(policy_mode: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let f = fixture("search.json")?;
        let policy = &f["policy"];
        Ok(Self {
            notes: f["notes"].as_array().ok_or("notes")?.clone(),
            policies: vec![(
                policy["domain"].as_str().ok_or("domain")?.to_owned(),
                policy_mode
                    .unwrap_or(policy["mode"].as_str().ok_or("mode")?)
                    .to_owned(),
            )],
            known: f["known_actors"]
                .as_array()
                .ok_or("actors")?
                .iter()
                .filter_map(|a| a.as_str().map(str::to_owned))
                .collect(),
            viewer: f["viewer_profile_id"].as_str().ok_or("viewer")?.to_owned(),
        })
    }

    fn apply(&mut self, event: &Value) -> TestResult {
        let op = event["op"].as_str().ok_or("op")?;
        if op == "add_policy" {
            self.policies.push((
                event["domain"].as_str().ok_or("domain")?.to_owned(),
                event["mode"].as_str().ok_or("mode")?.to_owned(),
            ));
            return Ok(());
        }
        let note = self
            .notes
            .iter_mut()
            .find(|n| n["note_id"] == event["note_id"])
            .ok_or("event note")?;
        match op {
            "edit" => {
                // An edit rewrites text only: created_at, and so the order, stays.
                if let Some(content) = event.get("content") {
                    note["content"] = content.clone();
                }
                if let Some(cw) = event.get("cw") {
                    note["cw"] = cw.clone();
                }
            }
            "delete" => note["deleted"] = json!(true),
            "revoke_view" => note["viewable"] = json!(false),
            _ => return Err(format!("unknown event {op}").into()),
        }
        Ok(())
    }

    fn controlled(&self, actor: &str) -> bool {
        self.policies
            .iter()
            .any(|(domain, _)| *domain == host(actor))
    }

    fn visible(&self, note: &Value) -> bool {
        let author = note["author"].as_str().unwrap_or_default();
        let quoted_controlled = note["kind"] == "quote"
            && note["quoted_author"]
                .as_str()
                .is_some_and(|quoted| self.controlled(quoted));
        note["viewable"] == true
            && note["deleted"] == false
            && note["kind"] != "repost"
            && !self.controlled(author)
            && !quoted_controlled
    }
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}

fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Reference-only cursor text. Production cursors are opaque and unspecified.
fn make_cursor(profile: &str, author: &str, q: &str, created_at: &str, note_id: &str) -> String {
    format!(
        "v1|{profile}|{}|{}|{created_at}|{note_id}",
        hex(author),
        hex(q)
    )
}

enum Outcome {
    Page(Value),
    Failure(u16, Value),
}

fn failure(
    status: ErrorCode,
    message: &str,
    field: Option<&str>,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    Ok(Outcome::Failure(
        status.status().as_u16(),
        error_body(status, message, field)?,
    ))
}

fn search(
    world: &World,
    profile: &str,
    query: &Value,
    cursor: Option<&str>,
    scan_budget: Option<usize>,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    let mut query = query.clone();
    if let Some(repeat) = query.get("q_repeat").cloned() {
        query.as_object_mut().ok_or("query")?.remove("q_repeat");
        query["q"] = json!(expand(&repeat)?);
    }
    if let Some(cursor) = cursor {
        query["cursor"] = json!(cursor);
    }
    let params: NoteSearchParams = serde_json::from_value(query)?;
    if let Err(problems) = params.validate() {
        return Ok(Outcome::Failure(
            400,
            serde_json::to_value(ErrorBody {
                code: ErrorCode::InvalidRequest,
                message: "invalid search request".into(),
                details: problems,
                domain_block: None,
            })?,
        ));
    }
    let author = params.author.as_ref().map_or("", |a| a.0.as_str());
    let q = params.q.as_deref().unwrap_or("");
    let mut after: Option<(String, String)> = None;
    if let Some(cursor) = &params.cursor {
        let parts: Vec<_> = cursor.split('|').collect();
        let [version, c_profile, c_author, c_q, created_at, note_id] = parts[..] else {
            return failure(
                ErrorCode::InvalidRequest,
                "malformed cursor",
                Some("cursor"),
            );
        };
        if version != "v1" || c_profile != profile || c_author != hex(author) || c_q != hex(q) {
            return failure(
                ErrorCode::InvalidRequest,
                "cursor was issued for another condition",
                Some("cursor"),
            );
        }
        after = Some((created_at.to_owned(), note_id.to_owned()));
    }
    if !author.is_empty() && !world.known.contains(author) {
        return failure(
            ErrorCode::NotFound,
            "author actor is not fetched",
            Some("author"),
        );
    }
    let key = |n: &Value| {
        (
            n["created_at"].as_str().unwrap_or_default().to_owned(),
            n["note_id"].as_str().unwrap_or_default().to_owned(),
        )
    };
    let matches = |n: &Value| {
        world.visible(n)
            && (author.is_empty() || n["author"] == author)
            && (q.is_empty()
                || n["content"].as_str().is_some_and(|c| c.contains(q))
                || n["cw"].as_str().is_some_and(|c| c.contains(q)))
    };
    // The scan walks every stored note newest first; the budget counts scanned
    // notes whether or not they are visible or matching.
    let mut candidates: Vec<&Value> = world.notes.iter().collect();
    candidates.sort_by_key(|n| std::cmp::Reverse(key(n)));
    if let Some(position) = &after {
        candidates.retain(|n| &key(n) < position);
    }
    let limit = params.limit.unwrap_or(NOTE_SEARCH_DEFAULT_LIMIT) as usize;
    let mut found: Vec<&Value> = Vec::new();
    let mut scanned = 0;
    let mut frontier: Option<(String, String)> = None;
    let mut truncated = false;
    let mut remaining = candidates.iter();
    for note in remaining.by_ref() {
        if scan_budget.is_some_and(|budget| scanned >= budget) {
            truncated = true;
            break;
        }
        scanned += 1;
        frontier = Some(key(note));
        if matches(note) {
            found.push(note);
            if found.len() == limit {
                break;
            }
        }
    }
    if truncated && scanned == 0 {
        return failure(
            ErrorCode::SearchTimeout,
            "no resumable progress before the time limit",
            None,
        );
    }
    // Determining whether another match exists is part of the same page
    // evaluation in this reference, so it is not charged to the budget.
    let more = !truncated && remaining.any(|n| matches(n));
    let items: Vec<Value> = found
        .iter()
        .map(|n| {
            let body = match n["cw"].as_str() {
                Some(cw) => json!({"display": "content_warning", "content_warning": cw}),
                None => json!({"display": "text", "content": n["content"]}),
            };
            json!({
                "kind": n["kind"],
                "note": {
                    "note_id": n["note_id"],
                    "author": n["author"],
                    "created_at": n["created_at"],
                    "body": body,
                },
            })
        })
        .collect();
    let mut page = json!({"truncated": truncated});
    let resume = if truncated {
        frontier
    } else if more {
        found.last().map(|last| key(last))
    } else {
        None
    };
    if let Some((created_at, note_id)) = resume {
        page["next_cursor"] = json!(make_cursor(profile, author, q, &created_at, &note_id));
    }
    Ok(Outcome::Page(
        json!({"profile_id": profile, "items": items, "page": page}),
    ))
}

fn summary(response: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    response["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| {
            Ok(json!({
                "note_id": item["note"]["note_id"],
                "kind": item["kind"],
                "display": item["note"]["body"]["display"],
            }))
        })
        .collect()
}

/// Check one outcome against the fixture's expectation; returns the next cursor.
fn assert_outcome(
    spec: &Value,
    outcome: Outcome,
    expect: &Value,
    label: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let status = expect["status"].as_u64().ok_or("status")? as u16;
    match outcome {
        Outcome::Page(response) => {
            assert_eq!(status, 200, "{label}");
            let items = summary(&response)?;
            assert_eq!(Value::Array(items), expect["items"], "{label}");
            let next = response["page"]["next_cursor"].as_str().map(str::to_owned);
            assert_eq!(next.is_some(), expect["next_cursor"] == true, "{label}");
            assert_eq!(
                response["page"]["truncated"] == true,
                expect["truncated"] == true,
                "{label}"
            );
            if response["page"]["truncated"] == true {
                assert!(next.is_some(), "{label}: a truncated page must resume");
            }
            check(spec, "NoteSearchResponse", &response)?;
            roundtrip::<NoteSearchResponse>(&response)?;
            // No trace of suppression or counts, and CW notes never carry a body.
            let encoded = serde_json::to_string(&response)?;
            for forbidden in [
                "total",
                "hidden",
                "suppress",
                "placeholder",
                "blocked.example",
            ] {
                assert!(!encoded.contains(forbidden), "{label}: {forbidden}");
            }
            for item in response["items"].as_array().ok_or("items")? {
                assert_ne!(item["kind"], "repost", "{label}");
                if item["note"]["body"]["display"] == "content_warning" {
                    assert!(item["note"]["body"].get("content").is_none(), "{label}");
                }
            }
            Ok(next)
        }
        Outcome::Failure(code, body) => {
            assert_eq!(code, status, "{label}");
            assert_eq!(body["code"], expect["code"], "{label}");
            if let Some(field) = expect.get("field") {
                assert_eq!(&body["details"][0]["field"], field, "{label}");
            }
            check(spec, "ErrorBody", &body)?;
            roundtrip::<ErrorBody>(&body)?;
            let parsed: ErrorBody = serde_json::from_value(body)?;
            assert_eq!(parsed.code.status().as_u16(), status, "{label}");
            Ok(None)
        }
    }
}

#[test]
fn reference_projection_satisfies_every_search_case() -> TestResult {
    let spec = spec()?;
    let f = fixture("search.json")?;
    let mut names = BTreeSet::new();
    let mut statuses = BTreeSet::new();
    for case in f["cases"].as_array().ok_or("cases")? {
        let name = case["name"].as_str().ok_or("name")?;
        assert!(names.insert(name));
        let world = World::load(case["policy_mode"].as_str())?;
        let outcome = search(
            &world,
            &world.viewer,
            &case["query"],
            None,
            case["scan_budget"].as_u64().map(|b| b as usize),
        )?;
        statuses.insert(case["expect"]["status"].as_u64().ok_or("status")?);
        assert_outcome(&spec, outcome, &case["expect"], name)?;
    }
    assert!(names.len() >= 25);
    assert_eq!(statuses, BTreeSet::from([200, 400, 404, 503]));
    Ok(())
}

#[test]
fn paging_cursor_binding_and_mid_page_changes_follow_the_contract() -> TestResult {
    let spec = spec()?;
    let f = fixture("search.json")?;
    let scenarios = f["scenarios"].as_array().ok_or("scenarios")?;
    assert!(scenarios.len() >= 5);
    for scenario in scenarios {
        let name = scenario["name"].as_str().ok_or("name")?;
        let mut world = World::load(None)?;
        let mut last_cursor: Option<String> = None;
        let mut returned = BTreeSet::new();
        let mut returned_in_order: Vec<String> = Vec::new();
        for (index, step) in scenario["steps"]
            .as_array()
            .ok_or("steps")?
            .iter()
            .enumerate()
        {
            let label = format!("{name} step {index}");
            for event in step["events"].as_array().into_iter().flatten() {
                world.apply(event)?;
            }
            let mut query = scenario["query"].clone();
            for (key, value) in step["query_patch"].as_object().into_iter().flatten() {
                query[key] = value.clone();
            }
            let cursor = match step["cursor"].as_str() {
                None => None,
                Some("prev") => Some(last_cursor.clone().ok_or("no previous cursor")?),
                Some(other) => Some(other.to_owned()),
            };
            let profile = step["profile_id"]
                .as_str()
                .unwrap_or(&world.viewer)
                .to_owned();
            let budget = step["scan_budget"].as_u64().map(|b| b as usize);
            let outcome = search(&world, &profile, &query, cursor.as_deref(), budget)?;
            if let Outcome::Page(response) = &outcome {
                for item in summary(response)? {
                    assert!(
                        returned.insert(item["note_id"].to_string()),
                        "{label}: a note was returned twice"
                    );
                    returned_in_order.push(item["note_id"].to_string());
                }
            }
            let is_page = matches!(outcome, Outcome::Page(_));
            let next = assert_outcome(&spec, outcome, &step["expect"], &label)?;
            if is_page {
                last_cursor = next;
            }
        }
        if scenario["complete_equals_untruncated"] == true {
            let mut full_query = scenario["query"].clone();
            full_query["limit"] = json!(NOTE_SEARCH_MAX_LIMIT);
            let Outcome::Page(full) = search(&world, &world.viewer, &full_query, None, None)?
            else {
                return Err("full run failed".into());
            };
            let expected: Vec<String> = summary(&full)?
                .iter()
                .map(|i| i["note_id"].to_string())
                .collect();
            assert_eq!(
                returned_in_order, expected,
                "{name}: partial pages are an exact prefix sequence, no gaps"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn search_route_stays_an_explicit_501_for_any_query() -> TestResult {
    let (app, _) = server::openapi::contract();
    for query in [
        "",
        "?q=hello",
        "?author=https%3A%2F%2Fremote.example%2Fusers%2Falice",
        "?q=&limit=0&cursor=garbage",
        "?author=%40alice%40remote.example",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/profiles/V1StGXR8_Z5jdHi6B-myT/notes/search{query}"
                    ))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{query}");
        let bytes = response.into_body().collect().await?.to_bytes();
        let parsed: ErrorBody = serde_json::from_slice(&bytes)?;
        assert_eq!(parsed.code, ErrorCode::NotImplemented);
    }
    Ok(())
}
