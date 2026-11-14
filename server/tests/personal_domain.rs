//! S8 contract evidence only. Reference projections below are TEST-ONLY;
//! production stubs do not consult policy, filter or accept/reject activities.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use jsonschema::Draft;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use server::api_v1::{ErrorBody, ErrorCode, personal_domain::*, quick_reaction::EmojiRef};
use std::{collections::BTreeSet, path::PathBuf};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

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

#[test]
fn schema_fixtures_validate_every_surface_and_operation() -> TestResult {
    let spec = spec()?;
    let cases = fixture("schema_cases.json")?;
    let mut names = BTreeSet::new();
    for case in cases.as_array().ok_or("cases")? {
        assert!(names.insert(case["name"].as_str().ok_or("case name")?));
        let name = case["schema"].as_str().ok_or("schema name")?;
        assert_eq!(
            valid(&spec, &component(&spec, name), &case["payload"])?,
            case["valid"].as_bool().ok_or("valid flag")?,
            "{}",
            case["name"]
        );
    }
    assert!(names.len() >= 70);
    Ok(())
}

#[test]
fn e8_projection_uses_the_same_identifiers_modes_and_revision() -> TestResult {
    let e8 = fixture("e8_contract.json")?;
    let spec = spec()?;
    assert_eq!(e8["source_commit"].as_str().ok_or("sha")?.len(), 40);
    let upstream = &e8["source_components"]["DomainControlResponse"]["properties"];
    let local = &spec["components"]["schemas"]["DomainControlRef"]["properties"];
    for field in ["profile_id", "domain"] {
        assert_eq!(upstream[field]["type"], "string");
        let target = local[field]["$ref"].as_str().ok_or("projection type")?;
        assert_eq!(
            spec.pointer(target.trim_start_matches('#'))
                .ok_or("reference")?["type"],
            "string"
        );
    }
    assert_eq!(upstream["revision"]["type"], local["revision"]["type"]);
    assert_eq!(local["revision"]["format"], "int64");
    // E8's generated minimum is 0, but its normative text/D8P starts at 1.
    // We constrain the read-only observation to that semantic lower bound.
    assert_eq!(local["revision"]["minimum"], 1);
    assert_eq!(
        spec["components"]["schemas"]["DomainControlMode"]["enum"],
        e8["source_components"]["DomainControlMode"]["enum"]
    );
    let projected = &e8["projection"];
    for field in ["profile_id", "domain", "mode", "revision"] {
        assert_eq!(projected[field], e8["response"][field]);
    }
    roundtrip::<DomainControlRef>(projected)?;
    check(&spec, "DomainControlRef", projected)?;
    let mut last = 0;
    for change in e8["revision_lifecycle"].as_array().ok_or("lifecycle")? {
        let revision = change["revision"].as_i64().ok_or("revision")?;
        assert_eq!(revision, last + 1);
        last = revision;
    }
    assert_eq!(last, projected["revision"].as_i64().ok_or("revision")?);
    let mut max = projected.clone();
    max["revision"] = json!(i64::MAX);
    roundtrip::<DomainControlRef>(&max)?;
    max["revision"] = json!(i64::MAX as u64 + 1);
    assert!(serde_json::from_value::<DomainControlRef>(max).is_err());
    let domain_doc = spec["components"]["schemas"]["ControlDomain"]["description"]
        .as_str()
        .ok_or("domain description")?;
    for text in [
        "UTS #46 non-transitional",
        "one trailing",
        "Exact match",
        "422",
        "canonical actor-ID",
    ] {
        assert!(domain_doc.contains(text), "missing E8 rule {text}");
    }
    Ok(())
}

fn assert_hidden(payload: &Value) -> TestResult {
    let object = payload.as_object().ok_or("hidden object")?;
    assert_eq!(
        object.keys().map(String::as_str).collect::<Vec<_>>(),
        ["domain", "mode", "revision", "state"]
    );
    assert_eq!(payload["state"], "hidden");
    assert!(!serde_json::to_string(payload)?.contains("SECRET_"));
    Ok(())
}

#[test]
fn hidden_schema_and_rust_deserialization_reject_every_content_extension() -> TestResult {
    let spec = spec()?;
    for name in ["NotePresentation", "EmbeddedNotePresentation"] {
        for variant in spec["components"]["schemas"][name]["oneOf"]
            .as_array()
            .ok_or("variants")?
        {
            assert_eq!(variant["additionalProperties"], false, "{name}");
        }
    }
    let cases = fixture("schema_cases.json")?;
    for case in cases.as_array().ok_or("cases")? {
        let name = case["name"].as_str().ok_or("name")?;
        if name.starts_with("hidden_forbids_") {
            assert!(
                serde_json::from_value::<DirectNoteResponse>(case["payload"].clone()).is_err(),
                "{name}"
            );
        } else if name.starts_with("embedded_forbids_") {
            assert!(
                serde_json::from_value::<EmbeddedNotePresentation>(case["payload"].clone())
                    .is_err(),
                "{name}"
            );
        } else if name.starts_with("hidden_") {
            roundtrip::<DirectNoteResponse>(&case["payload"])?;
            assert_hidden(&case["payload"]["presentation"])?;
        }
    }
    // A catch-all unknown field is forbidden, not merely a finite blacklist.
    Ok(())
}

fn actor_host(url: &Value) -> Result<String, Box<dyn std::error::Error>> {
    let url = reqwest::Url::parse(url.as_str().ok_or("canonical actor URL")?)?;
    Ok(url.host_str().ok_or("actor host")?.to_owned())
}

#[test]
fn surface_reference_fixtures_omit_whole_items_without_leaking_a_trace() -> TestResult {
    let spec = spec()?;
    let scenarios = fixture("surfaces.json")?;
    for scenario in scenarios.as_array().ok_or("surfaces")? {
        let profile = &scenario["profile_id"];
        let policy = &scenario["policy"];
        roundtrip::<DomainControlRef>(policy)?;
        let mut visible = Vec::new();
        for candidate in scenario["candidates"].as_array().ok_or("candidates")? {
            let applies = profile == &policy["profile_id"];
            let author_matches = actor_host(&candidate["actor_url"])? == policy["domain"];
            let target_matches = candidate
                .get("target_actor_url")
                .map(actor_host)
                .transpose()?
                .is_some_and(|d| d == policy["domain"]);
            // Claim/display domain is intentionally never consulted. Only
            // verified canonical hosts, and only one immediate quote target.
            if !(applies && (author_matches || target_matches)) {
                visible.push(candidate);
            }
        }
        let items: Vec<_> = visible.iter().map(|c| c["payload"].clone()).collect();
        let list = json!({"profile_id":profile,"items":items,"page":{}});
        assert_eq!(
            list, scenario["expected"]["timeline"],
            "{} timeline",
            scenario["name"]
        );
        assert!(scenario["expected"].get("search").is_none());
        check(&spec, "TimelineResponse", &list)?;
        roundtrip::<TimelineResponse>(&list)?;
        let notifications: Vec<_> = visible
            .iter()
            .map(|c| {
                let p = &c["payload"];
                let id = p
                    .get("note")
                    .map(|n| &n["note_id"])
                    .unwrap_or(&p["note_id"]);
                json!({"notification_id": id, "item":p, "unread":c["unread"]})
            })
            .collect();
        let unread = visible.iter().filter(|c| c["unread"] == true).count();
        let result =
            json!({"profile_id":profile,"items":notifications,"unread_count":unread,"page":{}});
        assert_eq!(result, scenario["expected"]["notifications"]);
        check(&spec, "ContentNotificationsResponse", &result)?;
        roundtrip::<ContentNotificationsResponse>(&result)?;
        if profile == &policy["profile_id"] {
            assert_eq!(visible.len(), 3); // includes same quoter's unrelated post and subdomain
            assert_eq!(unread, 2); // the four omitted notifications contribute nothing
            let encoded = serde_json::to_string(&scenario["expected"])?;
            for forbidden in [
                "SECRET_",
                "hidden",
                "placeholder",
                "suppression",
                "blocked.example",
                "revision",
            ] {
                assert!(!encoded.contains(forbidden), "{forbidden} leaked");
            }
        } else {
            assert_eq!(visible.len(), 7); // another Profile is unaffected
        }
    }
    Ok(())
}

fn permissions(blocked: bool, control: &Value) -> Value {
    let value = if blocked {
        json!({"status":"blocked", "control":control})
    } else {
        json!({"status":"allowed"})
    };
    json!({"reply":value,"quote":value,"repost":value,"like":value,"emoji_reaction":value})
}

#[test]
fn reveal_is_one_note_one_page_and_cannot_bypass_other_gates() -> TestResult {
    let spec = spec()?;
    let scenarios = fixture("reveal.json")?;
    for scenario in scenarios.as_array().ok_or("reveal")? {
        let policy = scenario["policy"].clone();
        let before = policy.clone();
        let hidden = json!({"state":"hidden","mode":policy["mode"],"domain":policy["domain"],"revision":policy["revision"]});
        let counts = fixture("counts.json")?["before"].clone();
        for step in scenario["steps"].as_array().ok_or("steps")? {
            let action = step["action"].as_str().ok_or("action")?;
            let blocked = policy["mode"] == "block";
            let presentation = match action {
                "get" | "get_other" | "hide" | "reload" | "revisit" => {
                    assert_hidden(&hidden)?;
                    hidden.clone()
                }
                "reveal" => {
                    json!({"state":"visible", "item":{"kind":"post", "note":{"note_id":step["note_id"],"content":"SECRET_BODY"}}, "operations":permissions(blocked,&policy),"reaction_counts":counts})
                }
                "reveal_quote" => {
                    json!({"state":"visible","item":{"kind":"quote","note":{"note_id":step["note_id"],"content":"SECRET_QUOTER_COMMENT"},"quoted_note":hidden}, "operations":permissions(false,&policy),"reaction_counts":counts})
                }
                "reveal_restricted" => {
                    json!({"state":"restricted","reason":step["restriction"],"operations":permissions(blocked,&policy)})
                }
                _ => return Err(format!("unknown action {action}").into()),
            };
            let response = json!({"profile_id":policy["profile_id"], "presentation":presentation});
            assert_eq!(response, step["expect"], "{} {action}", scenario["name"]);
            check(&spec, "DirectNoteResponse", &response)?;
            roundtrip::<DirectNoteResponse>(&response)?;
            if action == "reveal_quote" {
                assert_hidden(&response["presentation"]["item"]["quoted_note"])?;
                assert_eq!(
                    response["presentation"]["operations"]["reply"]["status"],
                    "allowed"
                );
            }
            if action == "reveal_restricted" {
                let encoded = serde_json::to_string(&response)?;
                assert!(!encoded.contains("SECRET_"));
                assert!(!encoded.contains("content"));
            }
            assert_eq!(
                policy, before,
                "reveal must never change Emumet policy/revision"
            );
        }
    }
    Ok(())
}

#[test]
fn every_action_has_separate_inbound_and_outbound_refusal_even_if_d13_allows() -> TestResult {
    let spec = spec()?;
    let fixtures = fixture("rejections.json")?;
    let mut operations = BTreeSet::new();
    for f in fixtures.as_array().ok_or("rejections")? {
        let operation = f["operation"].as_str().ok_or("operation")?;
        assert!(operations.insert(operation));
        assert_eq!(f["d13_permission"], "allow");
        let d13_allows = f["d13_permission"] == "allow";
        let block_applies = f["inbound"]["rejected"]["control"]["mode"] == "block";
        assert!(
            !(d13_allows && !block_applies),
            "D13 cannot bypass domain block"
        );
        assert_eq!(f["inbound"]["outcome"], "rejected");
        assert_eq!(f["outbound_status"], 403);
        assert_eq!(f["inbound"]["rejected"], f["outbound"]["domain_block"]);
        assert_eq!(f["outbound"]["domain_block"]["control"]["mode"], "block");
        check(
            &spec,
            f["inbound_schema"].as_str().ok_or("inbound schema")?,
            &f["inbound"],
        )?;
        check(
            &spec,
            f["details_schema"].as_str().ok_or("detail schema")?,
            &f["outbound"]["domain_block"],
        )?;
        check(&spec, "ErrorBody", &f["outbound"])?;
        roundtrip::<ErrorBody>(&f["outbound"])?;
        let (method, suffix) = match operation {
            "reply" => ("post", "replies"),
            "quote" => ("post", "quotes"),
            "repost" => ("post", "reposts"),
            "like" => ("put", "like"),
            "emoji_reaction" => ("put", "reaction"),
            _ => return Err("unknown post action".into()),
        };
        let path = format!("/api/v1/profiles/{{profile_nanoid}}/notes/{{note_id}}/{suffix}");
        let response_schema = &spec["paths"][path][method]["responses"]["403"]["content"]["application/json"]
            ["schema"];
        assert!(valid(&spec, response_schema, &f["outbound"])?);
        roundtrip::<OutboundDomainBlock>(&f["outbound"]["domain_block"])?;
        match operation {
            "reply" => roundtrip::<InboundReplyRejection>(&f["inbound"]),
            "quote" => roundtrip::<InboundQuoteRejection>(&f["inbound"]),
            "repost" => roundtrip::<InboundRepostRejection>(&f["inbound"]),
            "like" => roundtrip::<InboundLikeRejection>(&f["inbound"]),
            "emoji_reaction" => roundtrip::<InboundEmojiReactionRejection>(&f["inbound"]),
            _ => return Err("unexpected operation".into()),
        }?;
        // Only the emoji contract carries emoji identity: like != emoji.
        assert_eq!(
            f["outbound"]["domain_block"].get("emoji").is_some(),
            operation == "emoji_reaction"
        );
    }
    assert_eq!(operations.len(), 5);
    Ok(())
}

#[test]
fn accepted_reaction_counts_survive_block_and_refused_attempts_add_nothing() -> TestResult {
    let spec = spec()?;
    let f = fixture("counts.json")?;
    for name in ["before", "after_block", "after_rejections"] {
        check(&spec, "ReactionCounts", &f[name])?;
        roundtrip::<ReactionCounts>(&f[name])?;
    }
    assert_eq!(f["before"], f["after_block"]);
    assert_eq!(f["before"], f["after_rejections"]);
    let mut counts = f["before"].clone();
    for attempt in f["rejected_attempts"].as_array().ok_or("attempts")? {
        // Test-only aggregate projection: rejected attempts never emit an
        // accepted-reaction increment; they also never subtract past counts.
        if attempt["result"]["outcome"] == "accepted" {
            let target = if attempt["operation"] == "like" {
                &mut counts["likes"]
            } else {
                &mut counts["emoji"][0]["count"]
            };
            *target = json!(target.as_u64().ok_or("count")? + 1);
        }
        assert_eq!(attempt["result"]["outcome"], "rejected");
        assert_eq!(counts, f["after_rejections"]);
    }
    assert_eq!(counts["likes"], 7);
    assert_eq!(counts["emoji"][0]["count"], 3);
    assert_eq!(counts["emoji"][1]["count"], 2);
    Ok(())
}

#[test]
fn counts_fixtures_carry_a_view_for_every_custom_chip() -> TestResult {
    let f = fixture("counts.json")?;
    for name in ["before", "after_block", "after_rejections"] {
        let counts: ReactionCounts = serde_json::from_value(f[name].clone())?;
        for chip in &counts.emoji {
            chip.check_consistency()?;
        }
    }
    Ok(())
}

#[test]
fn external_emoji_chips_expose_their_origin_per_chip() -> TestResult {
    let spec = spec()?;
    let f = fixture("remote_emoji_chips.json")?;
    let external_ids = f["external_ids"].as_object().ok_or("external_ids")?;
    for (name, payload) in f["valid"].as_object().ok_or("valid")? {
        check(&spec, "ReactionCounts", payload)?;
        roundtrip::<ReactionCounts>(payload)?;
        let counts: ReactionCounts = serde_json::from_value(payload.clone())?;
        let mut seen = BTreeSet::new();
        let mut external = Vec::new();
        for chip in &counts.emoji {
            chip.check_consistency()
                .map_err(|e| format!("{name}: {e}"))?;
            assert!(
                seen.insert(serde_json::to_string(&chip.emoji)?),
                "{name}: duplicate chip"
            );
            // The globe mark is decided per chip from view.origin alone.
            if chip.view.as_ref().is_some_and(|v| v.origin.is_some()) {
                let EmojiRef::Custom { id } = &chip.emoji else {
                    return Err(format!("{name}: only custom emoji can be external").into());
                };
                external.push(id.clone());
            }
        }
        let expected: Vec<String> = serde_json::from_value(external_ids[name].clone())?;
        assert_eq!(external, expected, "{name}");
    }
    // Same shortcode on different servers: different chips told apart by origin.
    let same = &f["valid"]["same_name_different_origin"]["emoji"];
    let shortcodes: Vec<&Value> = same
        .as_array()
        .ok_or("chips")?
        .iter()
        .map(|c| &c["view"]["shortcode"])
        .collect();
    assert!(shortcodes.iter().all(|s| **s == json!("partycat")));
    let origins: BTreeSet<String> = same
        .as_array()
        .ok_or("chips")?
        .iter()
        .map(|c| c["view"]["origin"].to_string())
        .collect();
    assert_eq!(
        origins.len(),
        3,
        "remote, other and local (null) origins differ"
    );
    Ok(())
}

#[test]
fn invalid_external_emoji_chips_are_rejected() -> TestResult {
    let spec = spec()?;
    let f = fixture("remote_emoji_chips.json")?;
    for (name, payload) in f["schema_invalid"].as_object().ok_or("schema_invalid")? {
        assert!(
            !valid(&spec, &component(&spec, "ReactionCounts"), payload)?,
            "{name} must violate the schema"
        );
    }
    for (name, payload) in f["schema_valid_but_inconsistent"]
        .as_object()
        .ok_or("inconsistent")?
    {
        check(&spec, "ReactionCounts", payload)?;
        let counts: ReactionCounts = serde_json::from_value(payload.clone())?;
        assert!(
            counts
                .emoji
                .iter()
                .any(|chip| chip.check_consistency().is_err()),
            "{name} must break the chip rules"
        );
    }
    Ok(())
}

#[test]
fn s3_request_and_existing_responses_remain_compatible() -> TestResult {
    let spec = spec()?;
    let op = &spec["paths"]["/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction"]["put"];
    assert_eq!(op["operationId"], "putNoteReaction");
    assert_eq!(
        op["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/PutReaction"
    );
    assert_eq!(
        op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ReactionState"
    );
    let forbidden = &op["responses"]["403"]["content"]["application/json"]["schema"];
    assert_eq!(forbidden["$ref"], "#/components/schemas/ErrorBody");
    let legacy_error = json!({"code":"forbidden", "message":"not your Profile"});
    assert!(valid(&spec, forbidden, &legacy_error)?);
    assert_eq!(
        serde_json::from_value::<ErrorBody>(legacy_error)?.domain_block,
        None
    );
    let refusals = fixture("rejections.json")?;
    let emoji = refusals
        .as_array()
        .ok_or("refusals")?
        .iter()
        .find(|f| f["operation"] == "emoji_reaction")
        .ok_or("emoji")?;
    assert!(valid(&spec, forbidden, &emoji["outbound"])?);
    check(
        &spec,
        "PutReaction",
        &json!({"emoji":emoji["outbound"]["domain_block"]["emoji"]}),
    )?;
    Ok(())
}

#[tokio::test]
async fn all_nine_s8_routes_and_s3_reaction_put_are_always_structured_501() -> TestResult {
    let (app, doc) = server::openapi::contract();
    let spec = serde_json::to_value(doc)?;
    let mut count = 0;
    for (path, item) in spec["paths"].as_object().ok_or("paths")? {
        for (method, op) in item.as_object().ok_or("operations")? {
            let is_s8 = op["tags"]
                .as_array()
                .is_some_and(|tags| tags.contains(&json!("personal-domain-content")));
            let is_s3_put = op["operationId"] == "putNoteReaction";
            if !is_s8 && !is_s3_put {
                continue;
            }
            if is_s8 {
                count += 1;
            }
            assert!(op["responses"]["501"].is_object());
            let uri = path
                .replace("{profile_nanoid}", "invalid-profile!")
                .replace("{note_id}", "not-a-uuid");
            for body in ["", "{ invalid json", "{\"content\":\"hello\"}"] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method.to_uppercase().as_str())
                            .uri(&uri)
                            .header("content-type", "text/plain")
                            .body(Body::from(body))?,
                    )
                    .await?;
                assert_eq!(
                    response.status(),
                    StatusCode::NOT_IMPLEMENTED,
                    "{method} {path}"
                );
                let bytes = response.into_body().collect().await?.to_bytes();
                let parsed: ErrorBody = serde_json::from_slice(&bytes)?;
                assert_eq!(parsed.code, ErrorCode::NotImplemented);
                assert!(parsed.domain_block.is_none());
            }
        }
    }
    assert_eq!(count, 9);
    Ok(())
}

#[test]
fn s8_remains_contract_only_and_does_not_add_an_inbox() -> TestResult {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/api_v1/personal_domain.rs"),
    )?;
    assert!(!source.contains("todo!("));
    assert!(!source.contains("unimplemented!("));
    assert_eq!(source.matches("ApiV1Error::not_implemented()").count(), 9);
    let spec = spec()?;
    for (path, item) in spec["paths"].as_object().ok_or("paths")? {
        assert!(!path.contains("inbox"));
        assert!(!path.contains("permission"));
        for op in item.as_object().ok_or("operation")?.values() {
            if op["tags"]
                .as_array()
                .is_some_and(|tags| tags.contains(&json!("personal-domain-content")))
            {
                let description = format!(
                    "{} {}",
                    op["summary"].as_str().unwrap_or_default(),
                    op["description"].as_str().unwrap_or_default()
                );
                assert!(description.contains("Contract only"));
                assert_eq!(op["security"], json!([{"bearer_auth":[]}]));
            }
        }
    }
    Ok(())
}
