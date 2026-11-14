//! Contract tests for the quick reaction API (`quick-reaction-api-contract`).
//!
//! * type tests: serialization and validation of the DTOs;
//! * schema tests: the generated schema states the same constraints;
//! * fixture tests: `tests/fixtures/quick_reaction/schema_cases.json` is checked
//!   against the generated schema and the Rust validation, and every scenario in
//!   `scenarios/` is replayed against a small reference model of the documented
//!   semantics (retry, conflict, unavailable emoji) with each expected response
//!   also validated against the schema;
//! * route tests: every contract route answers an explicit 501.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use jsonschema::Draft;
use serde::Deserialize;
use serde_json::{Value, json};
use server::api_v1::{
    ApiV1Error, ErrorBody, ErrorCode,
    personal_domain::ControlDomain,
    quick_reaction::{
        EmojiAvailability, EmojiImageState, EmojiRef, EmojiUnavailableReason, EmojiUsage,
        EmojiView, MAX_FAVORITES, ProfileNanoid, PutReaction, QuickReactionSettings, ReactionState,
        RevisionConflictBody, UpdateQuickReactionSettings,
    },
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tower::ServiceExt;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PROFILE: &str = "V1StGXR8_Z5jdHi6B-myT";
const NOTE: &str = "0f0a5d3e-8f3c-4c53-9b64-3f5f8f0f2a11";

fn unicode(value: &str) -> EmojiRef {
    EmojiRef::Unicode {
        value: value.into(),
    }
}

fn custom(id: &str) -> EmojiRef {
    EmojiRef::Custom { id: id.into() }
}

fn spec() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&server::openapi::render()?)?)
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quick_reaction")
}

fn read_fixture(path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        fixture_dir().join(path),
    )?)?)
}

// ---------------------------------------------------------------- types ----

#[test]
fn emoji_ref_serializes_with_a_type_tag() -> TestResult {
    assert_eq!(
        serde_json::to_value(unicode("👍"))?,
        json!({"type": "unicode", "value": "👍"})
    );
    assert_eq!(
        serde_json::to_value(custom("blobcat"))?,
        json!({"type": "custom", "id": "blobcat"})
    );
    let parsed: EmojiRef = serde_json::from_value(json!({"type": "custom", "id": "blobcat"}))?;
    assert_eq!(parsed, custom("blobcat"));
    assert!(
        serde_json::from_value::<EmojiRef>(json!({"type": "custom", "id": "a", "x": 1})).is_err()
    );
    assert!(serde_json::from_value::<EmojiRef>(json!({"type": "remote", "id": "a"})).is_err());
    Ok(())
}

#[test]
fn emoji_view_omits_absent_display_data() -> TestResult {
    let unicode_view = EmojiView {
        emoji: unicode("👍"),
        availability: EmojiAvailability::Available,
        unavailable_reason: None,
        shortcode: None,
        image_url: None,
        origin: None,
        image_state: None,
        usage: None,
    };
    assert_eq!(
        serde_json::to_value(&unicode_view)?,
        json!({"emoji": {"type": "unicode", "value": "👍"}, "availability": "available"})
    );
    let gone = EmojiView {
        emoji: custom("oldcat"),
        availability: EmojiAvailability::Unavailable,
        unavailable_reason: Some(EmojiUnavailableReason::Deleted),
        shortcode: Some("oldcat".into()),
        image_url: None,
        origin: None,
        image_state: None,
        usage: None,
    };
    assert_eq!(
        serde_json::to_value(&gone)?,
        json!({
            "emoji": {"type": "custom", "id": "oldcat"},
            "availability": "unavailable",
            "unavailable_reason": "deleted",
            "shortcode": "oldcat"
        })
    );
    Ok(())
}

#[test]
fn reaction_state_keeps_the_emoji_key_when_unreacted() -> TestResult {
    let state = ReactionState {
        note_id: Uuid::parse_str(NOTE)?,
        emoji: None,
    };
    assert_eq!(
        serde_json::to_value(&state)?,
        json!({"note_id": NOTE, "emoji": null})
    );
    assert!(serde_json::from_value::<ReactionState>(json!({"note_id": NOTE})).is_ok());
    Ok(())
}

#[test]
fn favorites_count_is_validated() -> TestResult {
    let emoji = ["👍", "❤️", "🎉", "🔥", "👀", "😂"];
    for count in 0..=MAX_FAVORITES {
        let body = UpdateQuickReactionSettings {
            expected_revision: 0,
            favorites: emoji[..count].iter().map(|e| unicode(e)).collect(),
        };
        assert!(
            body.validate().is_ok(),
            "{count} favorites must be accepted"
        );
    }
    let six = UpdateQuickReactionSettings {
        expected_revision: 0,
        favorites: emoji.iter().map(|e| unicode(e)).collect(),
    };
    let problems = six.validate().err().ok_or("6 favorites must be rejected")?;
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].field, "/favorites");
    Ok(())
}

#[test]
fn duplicates_are_reported_at_the_later_index() -> TestResult {
    let body = UpdateQuickReactionSettings {
        expected_revision: 1,
        favorites: vec![unicode("👍"), custom("a"), unicode("👍"), custom("a")],
    };
    let problems = body.validate().err().ok_or("duplicates must be rejected")?;
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert_eq!(fields, ["/favorites/2", "/favorites/3"]);
    let tones = UpdateQuickReactionSettings {
        expected_revision: 1,
        favorites: vec![unicode("👍"), unicode("👍🏽")],
    };
    assert!(tones.validate().is_ok(), "skin tones are distinct emoji");
    Ok(())
}

#[test]
fn emoji_identifier_formats_are_validated() {
    for bad in [
        unicode(""),
        unicode("a"),
        unicode("1"),
        unicode("👍 "),
        unicode("👍\n"),
        unicode(&"👍".repeat(33)),
        custom(""),
        custom("bad id"),
        custom("日本語"),
        custom(&"a".repeat(129)),
    ] {
        assert!(bad.validate("/emoji").is_err(), "{bad:?} must be rejected");
    }
    for good in [
        unicode("👍"),
        unicode("❤️"),
        unicode("👨‍👩‍👧‍👦"),
        unicode("🇯🇵"),
        unicode("1️⃣"),
        custom("blobcat"),
        custom("remote.example:blob_cat@2-b"),
        custom(&"a".repeat(128)),
    ] {
        assert!(good.validate("/emoji").is_ok(), "{good:?} must be accepted");
    }
}

#[test]
fn profile_nanoid_format_is_validated() {
    for good in [PROFILE, "a", &"x".repeat(64)] {
        assert!(ProfileNanoid(good.into()).validate("/p").is_ok(), "{good}");
    }
    for bad in ["", "has space", "slash/inside", "日本", &"x".repeat(65)] {
        assert!(ProfileNanoid(bad.into()).validate("/p").is_err(), "{bad}");
    }
}

#[test]
fn request_bodies_reject_unknown_fields() {
    assert!(
        serde_json::from_value::<UpdateQuickReactionSettings>(
            json!({"expected_revision": 0, "favorites": [], "x": 1})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<PutReaction>(
            json!({"emoji": {"type": "unicode", "value": "👍"}, "x": 1})
        )
        .is_err()
    );
}

#[test]
fn revision_conflict_body_serializes_with_current_revision() -> TestResult {
    assert_eq!(
        serde_json::to_value(RevisionConflictBody::new(5))?,
        json!({
            "code": "revision_conflict",
            "message": "expected_revision does not match the stored revision",
            "current_revision": 5
        })
    );
    assert_eq!(ErrorCode::RevisionConflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        ErrorCode::EmojiUnavailable.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::EmojiUnavailable)?,
        json!("emoji_unavailable")
    );
    Ok(())
}

fn external_view() -> EmojiView {
    EmojiView {
        emoji: custom("partycat@remote.example"),
        availability: EmojiAvailability::Available,
        unavailable_reason: None,
        shortcode: Some("partycat".into()),
        image_url: Some("https://remote.example/emoji/partycat.png".into()),
        origin: Some(ControlDomain("remote.example".into())),
        image_state: Some(EmojiImageState::Present),
        usage: Some(EmojiUsage::ExistingReactionOnly),
    }
}

#[test]
fn external_emoji_view_serializes_origin_image_state_and_usage() -> TestResult {
    assert_eq!(
        serde_json::to_value(external_view())?,
        json!({
            "emoji": {"type": "custom", "id": "partycat@remote.example"},
            "availability": "available",
            "shortcode": "partycat",
            "image_url": "https://remote.example/emoji/partycat.png",
            "origin": "remote.example",
            "image_state": "present",
            "usage": "existing_reaction_only"
        })
    );
    let parsed: EmojiView = serde_json::from_value(json!({
        "emoji": {"type": "custom", "id": "ghostcat@remote.example"},
        "availability": "available",
        "shortcode": "ghostcat",
        "origin": "remote.example",
        "image_state": "missing",
        "usage": "existing_reaction_only"
    }))?;
    assert_eq!(parsed.image_state, Some(EmojiImageState::Missing));
    assert_eq!(parsed.availability, EmojiAvailability::Available);
    assert!(parsed.image_url.is_none());
    parsed.check_consistency()?;
    // S3 era views (no new fields) still parse.
    let legacy: EmojiView = serde_json::from_value(
        json!({"emoji": {"type": "custom", "id": "blobcat"}, "availability": "available"}),
    )?;
    assert!(legacy.origin.is_none() && legacy.image_state.is_none() && legacy.usage.is_none());
    assert!(serde_json::from_value::<EmojiUsage>(json!("everywhere")).is_err());
    assert!(serde_json::from_value::<EmojiImageState>(json!("broken")).is_err());
    Ok(())
}

#[test]
fn emoji_view_consistency_rules() -> TestResult {
    external_view().check_consistency()?;
    let mut anywhere = external_view();
    anywhere.usage = Some(EmojiUsage::Anywhere);
    assert!(
        anywhere.check_consistency().is_err(),
        "external must be existing_reaction_only"
    );
    let mut no_usage = external_view();
    no_usage.usage = None;
    assert!(no_usage.check_consistency().is_err());
    let mut no_url = external_view();
    no_url.image_url = None;
    assert!(
        no_url.check_consistency().is_err(),
        "present needs image_url"
    );
    let mut missing_with_url = external_view();
    missing_with_url.image_state = Some(EmojiImageState::Missing);
    assert!(missing_with_url.check_consistency().is_err());
    // image state is independent of availability
    let mut missing = external_view();
    missing.image_state = Some(EmojiImageState::Missing);
    missing.image_url = None;
    missing.check_consistency()?;
    let mut gone = missing.clone();
    gone.availability = EmojiAvailability::Unavailable;
    assert!(
        gone.check_consistency().is_err(),
        "unavailable needs a reason"
    );
    gone.unavailable_reason = Some(EmojiUnavailableReason::Deleted);
    gone.check_consistency()?;
    let mut unicode_with_origin = EmojiView {
        emoji: unicode("👍"),
        availability: EmojiAvailability::Available,
        unavailable_reason: None,
        shortcode: None,
        image_url: None,
        origin: None,
        image_state: None,
        usage: None,
    };
    unicode_with_origin.check_consistency()?;
    unicode_with_origin.origin = Some(ControlDomain("remote.example".into()));
    assert!(unicode_with_origin.check_consistency().is_err());
    Ok(())
}

#[test]
fn emoji_usage_not_allowed_is_a_422_error_code() -> TestResult {
    assert_eq!(
        serde_json::to_value(ErrorCode::EmojiUsageNotAllowed)?,
        json!("emoji_usage_not_allowed")
    );
    assert_eq!(
        ErrorCode::EmojiUsageNotAllowed.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let body: ErrorBody = serde_json::from_value(
        json!({"code": "emoji_usage_not_allowed", "message": "external emoji"}),
    )?;
    assert_eq!(body.code, ErrorCode::EmojiUsageNotAllowed);
    Ok(())
}

// --------------------------------------------------------------- schema ----

#[test]
fn schema_states_the_external_emoji_fields() -> TestResult {
    let spec = spec()?;
    let schemas = &spec["components"]["schemas"];
    let view = &schemas["EmojiView"];
    for field in ["origin", "image_state", "usage"] {
        assert!(view["properties"][field].is_object(), "EmojiView.{field}");
    }
    assert_eq!(
        view["required"],
        json!(["emoji", "availability"]),
        "the new fields are optional"
    );
    assert_eq!(
        schemas["EmojiImageState"]["enum"],
        json!(["present", "missing"])
    );
    assert_eq!(
        schemas["EmojiUsage"]["enum"],
        json!(["existing_reaction_only", "anywhere"])
    );
    let refs = |schema: &Value| {
        serde_json::to_string(schema)
            .map(|text| text.contains("#/components/schemas/ControlDomain"))
    };
    assert!(refs(&view["properties"]["origin"])?);
    // EmojiRef is unchanged: still exactly the two closed variants.
    let variants = schemas["EmojiRef"]["oneOf"]
        .as_array()
        .ok_or("EmojiRef oneOf")?;
    assert_eq!(variants.len(), 2);
    for (variant, (tag, field)) in variants
        .iter()
        .zip([("unicode", "value"), ("custom", "id")])
    {
        assert_eq!(variant["properties"]["type"]["enum"], json!([tag]));
        let mut names: Vec<&str> = variant["properties"]
            .as_object()
            .ok_or("properties")?
            .keys()
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        let mut expected = vec!["type", field];
        expected.sort_unstable();
        assert_eq!(names, expected);
    }
    assert!(
        schemas["ErrorCode"]["enum"]
            .as_array()
            .ok_or("ErrorCode enum")?
            .contains(&json!("emoji_usage_not_allowed"))
    );
    let count = &schemas["EmojiReactionCount"];
    assert!(count["properties"]["view"].is_object());
    assert_eq!(count["required"], json!(["emoji", "count"]));
    Ok(())
}

#[test]
fn reaction_put_documents_every_outcome_of_the_check_order() -> TestResult {
    let spec = spec()?;
    let op = &spec["paths"]["/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction"]["put"];
    let description = op["description"].as_str().ok_or("description")?;
    for needle in [
        "emoji_unavailable",
        "emoji_usage_not_allowed",
        "409",
        "no-op",
    ] {
        assert!(
            description.contains(needle),
            "operation text misses {needle}"
        );
    }
    assert_eq!(
        op["responses"]["409"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ErrorBody"
    );
    let settings =
        &spec["paths"]["/api/v1/profiles/{profile_nanoid}/quick-reaction-settings"]["put"];
    let text = settings["responses"]["422"]["description"]
        .as_str()
        .ok_or("422")?;
    assert!(text.contains("emoji_usage_not_allowed"));
    // Reading and deleting stay untouched by the external emoji rules.
    let delete =
        &spec["paths"]["/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction"]["delete"];
    assert!(delete["responses"]["409"].is_null() && delete["responses"]["422"].is_null());
    Ok(())
}

fn schema_of(spec: &Value, operation: &str, target: &Value) -> Option<Value> {
    for item in spec["paths"].as_object()?.values() {
        for op in item.as_object()?.values() {
            if op["operationId"] != operation {
                continue;
            }
            let schema = if target["kind"] == "request" {
                op["requestBody"]["content"]["application/json"]["schema"].clone()
            } else {
                let status = target["status"].as_str()?;
                op["responses"][status]["content"]["application/json"]["schema"].clone()
            };
            return (!schema.is_null()).then_some(schema);
        }
    }
    None
}

fn is_valid_against(spec: &Value, schema: &Value, instance: &Value) -> Result<bool, String> {
    let root = json!({"components": spec["components"], "allOf": [schema]});
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .build(&root)
        .map_err(|e| e.to_string())?;
    Ok(validator.is_valid(instance))
}

#[test]
fn schema_states_the_favorites_constraints() -> TestResult {
    let spec = spec()?;
    let favorites =
        &spec["components"]["schemas"]["UpdateQuickReactionSettings"]["properties"]["favorites"];
    assert_eq!(favorites["maxItems"], 5);
    assert_eq!(favorites["uniqueItems"], true);
    assert_eq!(favorites["items"]["$ref"], "#/components/schemas/EmojiRef");
    let stored = &spec["components"]["schemas"]["QuickReactionSettings"]["properties"]["favorites"];
    assert_eq!(stored["maxItems"], 5);
    assert_eq!(
        spec["components"]["schemas"]["QuickReactionSettings"]["required"],
        json!(["revision", "favorites"])
    );
    assert_eq!(
        spec["components"]["schemas"]["UpdateQuickReactionSettings"]["required"],
        json!(["expected_revision", "favorites"])
    );
    for variant in spec["components"]["schemas"]["EmojiRef"]["oneOf"]
        .as_array()
        .ok_or("EmojiRef must be a oneOf")?
    {
        assert_eq!(variant["additionalProperties"], false);
    }
    assert_eq!(
        spec["components"]["schemas"]["ReactionState"]["required"],
        json!(["note_id", "emoji"])
    );
    Ok(())
}

#[test]
fn every_operation_is_authenticated_and_documents_its_errors() -> TestResult {
    let spec = spec()?;
    let expected: BTreeMap<&str, &[&str]> = BTreeMap::from([
        (
            "getQuickReactionSettings",
            &["200", "400", "401", "403", "404", "501"][..],
        ),
        (
            "putQuickReactionSettings",
            &["200", "400", "401", "403", "404", "409", "422", "501"][..],
        ),
        (
            "getNoteReaction",
            &["200", "400", "401", "403", "404", "501"][..],
        ),
        (
            "putNoteReaction",
            &["200", "400", "401", "403", "404", "409", "422", "501"][..],
        ),
        (
            "deleteNoteReaction",
            &["200", "400", "401", "403", "404", "501"][..],
        ),
    ]);
    let mut seen = 0;
    for item in spec["paths"].as_object().ok_or("paths")?.values() {
        for op in item.as_object().ok_or("path item")?.values() {
            let id = op["operationId"].as_str().ok_or("operationId")?;
            let Some(statuses) = expected.get(id) else {
                continue;
            };
            seen += 1;
            assert_eq!(op["security"], json!([{"bearer_auth": []}]), "{id}");
            let mut documented: Vec<&str> = op["responses"]
                .as_object()
                .ok_or("responses")?
                .keys()
                .map(String::as_str)
                .collect();
            documented.sort_unstable();
            assert_eq!(&documented, statuses, "{id}");
            assert_eq!(
                op["responses"]["409"]["content"]["application/json"]["schema"]["$ref"]
                    .as_str()
                    .map(|s| s.to_owned()),
                match id {
                    "putQuickReactionSettings" =>
                        Some("#/components/schemas/RevisionConflictBody".to_owned()),
                    "putNoteReaction" => Some("#/components/schemas/ErrorBody".to_owned()),
                    _ => None,
                },
                "{id}"
            );
        }
    }
    assert_eq!(seen, expected.len());
    assert_eq!(
        spec["components"]["securitySchemes"]["bearer_auth"]["scheme"],
        "bearer"
    );
    Ok(())
}

#[test]
fn path_parameters_use_the_shared_identifiers() -> TestResult {
    let spec = spec()?;
    let op = &spec["paths"]["/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction"]["put"];
    let params = op["parameters"].as_array().ok_or("parameters")?;
    assert_eq!(params[0]["name"], "profile_nanoid");
    assert_eq!(
        params[0]["schema"]["$ref"],
        "#/components/schemas/ProfileNanoid"
    );
    assert_eq!(params[1]["name"], "note_id");
    assert_eq!(params[1]["schema"]["format"], "uuid");
    Ok(())
}

#[test]
fn legacy_development_reaction_api_is_not_in_the_schema() -> TestResult {
    let text = server::openapi::render()?;
    assert!(!text.contains("/api/notes"), "legacy dev route leaked");
    assert!(!text.contains("/api/timeline"));
    Ok(())
}

// ------------------------------------------------------------- fixtures ----

#[derive(Deserialize)]
struct SchemaCase {
    name: String,
    target: Value,
    body: Value,
    schema_valid: bool,
    rust_valid: Option<bool>,
}

fn rust_accepts(operation: &str, body: &Value) -> bool {
    match operation {
        "putQuickReactionSettings" => {
            serde_json::from_value::<UpdateQuickReactionSettings>(body.clone())
                .is_ok_and(|b| b.validate().is_ok())
        }
        "putNoteReaction" => {
            serde_json::from_value::<PutReaction>(body.clone()).is_ok_and(|b| b.validate().is_ok())
        }
        other => panic!("no request type for {other}"),
    }
}

#[test]
fn schema_cases_match_the_generated_schema() -> TestResult {
    let spec = spec()?;
    let cases: Vec<SchemaCase> = serde_json::from_value(read_fixture("schema_cases.json")?)?;
    assert!(cases.len() >= 40);
    let mut counts = (0, 0);
    for case in &cases {
        let operation = case.target["operation"].as_str().ok_or("operation")?;
        let schema = schema_of(&spec, operation, &case.target)
            .ok_or_else(|| format!("{}: no schema for {operation}", case.name))?;
        let valid = is_valid_against(&spec, &schema, &case.body)?;
        assert_eq!(valid, case.schema_valid, "schema: {}", case.name);
        if case.schema_valid {
            counts.0 += 1;
        } else {
            counts.1 += 1;
        }
        if case.target["kind"] == "request" {
            assert_eq!(
                rust_accepts(operation, &case.body),
                case.rust_valid.unwrap_or(case.schema_valid),
                "rust validation: {}",
                case.name
            );
        }
    }
    assert!(counts.0 >= 15 && counts.1 >= 20, "{counts:?}");
    Ok(())
}

/// The cases listed in the acceptance criteria must exist in the fixtures.
#[test]
fn schema_cases_cover_the_required_situations() -> TestResult {
    let cases: Vec<SchemaCase> = serde_json::from_value(read_fixture("schema_cases.json")?)?;
    let favorites_len = |case: &SchemaCase| {
        case.body["favorites"]
            .as_array()
            .map(Vec::len)
            .filter(|_| case.target["operation"] == "putQuickReactionSettings")
            .filter(|_| case.target["kind"] == "request")
    };
    for len in [0, 5, 6] {
        assert!(
            cases.iter().any(|c| favorites_len(c) == Some(len)),
            "no request fixture with {len} favorites"
        );
    }
    let has = |needle: &str| cases.iter().any(|c| c.name.contains(needle));
    for needle in [
        "duplicate",
        "revision_conflict",
        "emoji_unavailable",
        "unavailable",
    ] {
        assert!(has(needle), "no fixture mentions {needle}");
    }
    for status in ["400", "401", "403", "404", "409", "422", "501"] {
        assert!(
            cases
                .iter()
                .any(|c| c.schema_valid && c.target["status"] == status),
            "no valid response fixture for status {status}"
        );
    }
    Ok(())
}

// ------------------------------------------------------ reference model ----

#[derive(Deserialize, Clone)]
struct CustomEmojiSetup {
    shortcode: Option<String>,
    image_url: Option<String>,
    availability: EmojiAvailability,
    reason: Option<EmojiUnavailableReason>,
    origin: Option<ControlDomain>,
    image_state: Option<EmojiImageState>,
    usage: Option<EmojiUsage>,
}

#[derive(Deserialize)]
struct SettingsSetup {
    revision: u64,
    favorites: Vec<EmojiRef>,
}

#[derive(Deserialize)]
struct Setup {
    emoji: BTreeMap<String, CustomEmojiSetup>,
    settings: SettingsSetup,
    reaction: Option<EmojiRef>,
    /// Emoji other people reacted to the Note with (the chips that exist
    /// besides the Profile's own selection).
    #[serde(default)]
    chips: Vec<EmojiRef>,
}

/// Executable statement of the documented semantics for one Profile and one
/// Note. It exists only to check that the fixtures are consistent with the
/// contract text; it is not the (not yet existing) implementation.
struct Model {
    emoji: BTreeMap<String, CustomEmojiSetup>,
    revision: u64,
    favorites: Vec<EmojiRef>,
    reaction: Option<EmojiRef>,
    chips: Vec<EmojiRef>,
}

impl Model {
    fn new(setup: Setup) -> Self {
        Self {
            emoji: setup.emoji,
            revision: setup.settings.revision,
            favorites: setup.settings.favorites,
            reaction: setup.reaction,
            chips: setup.chips,
        }
    }

    fn external(&self, emoji: &EmojiRef) -> bool {
        match emoji {
            EmojiRef::Unicode { .. } => false,
            EmojiRef::Custom { id } => self
                .emoji
                .get(id)
                .is_some_and(|e| e.usage == Some(EmojiUsage::ExistingReactionOnly)),
        }
    }

    fn available(&self, emoji: &EmojiRef) -> bool {
        match emoji {
            EmojiRef::Unicode { .. } => true,
            EmojiRef::Custom { id } => self
                .emoji
                .get(id)
                .is_some_and(|e| e.availability == EmojiAvailability::Available),
        }
    }

    fn view(&self, emoji: &EmojiRef) -> EmojiView {
        let EmojiRef::Custom { id } = emoji else {
            return EmojiView {
                emoji: emoji.clone(),
                availability: EmojiAvailability::Available,
                unavailable_reason: None,
                shortcode: None,
                image_url: None,
                origin: None,
                image_state: None,
                usage: None,
            };
        };
        match self.emoji.get(id) {
            Some(entry) => EmojiView {
                emoji: emoji.clone(),
                availability: entry.availability,
                unavailable_reason: entry.reason,
                shortcode: entry.shortcode.clone(),
                image_url: entry.image_url.clone(),
                origin: entry.origin.clone(),
                image_state: entry.image_state,
                usage: entry.usage,
            },
            None => EmojiView {
                emoji: emoji.clone(),
                availability: EmojiAvailability::Unavailable,
                unavailable_reason: Some(EmojiUnavailableReason::Deleted),
                shortcode: None,
                image_url: None,
                origin: None,
                image_state: None,
                usage: None,
            },
        }
    }

    fn settings(&self) -> QuickReactionSettings {
        QuickReactionSettings {
            revision: self.revision,
            favorites: self.favorites.iter().map(|e| self.view(e)).collect(),
        }
    }

    fn reaction_state(&self) -> Result<ReactionState, uuid::Error> {
        Ok(ReactionState {
            note_id: Uuid::parse_str(NOTE)?,
            emoji: self.reaction.as_ref().map(|e| self.view(e)),
        })
    }

    fn unavailable_error(fields: Vec<String>) -> ErrorBody {
        ErrorBody {
            code: ErrorCode::EmojiUnavailable,
            domain_block: None,
            message: "an emoji in the request is unavailable".into(),
            details: fields
                .into_iter()
                .map(|field| server::api_v1::FieldViolation {
                    field,
                    message: "emoji is unavailable".into(),
                })
                .collect(),
        }
    }

    fn usage_error(code: ErrorCode, message: &str, fields: Vec<String>) -> ErrorBody {
        ErrorBody {
            code,
            domain_block: None,
            message: message.into(),
            details: fields
                .into_iter()
                .map(|field| server::api_v1::FieldViolation {
                    field,
                    message: message.into(),
                })
                .collect(),
        }
    }

    fn handle(&mut self, operation: &str, body: Option<&Value>) -> Result<(u16, Value), String> {
        let invalid = |details| {
            (
                400,
                serde_json::to_value(ApiV1Error::invalid_request(details).0),
            )
        };
        let respond = |status: u16, value: Result<Value, serde_json::Error>| {
            value.map(|v| (status, v)).map_err(|e| e.to_string())
        };
        match operation {
            "getQuickReactionSettings" => respond(200, serde_json::to_value(self.settings())),
            "putQuickReactionSettings" => {
                let request: UpdateQuickReactionSettings =
                    serde_json::from_value(body.cloned().ok_or("body required")?)
                        .map_err(|e| e.to_string())?;
                if let Err(details) = request.validate() {
                    let (status, value) = invalid(details);
                    return respond(status, value);
                }
                if request.favorites == self.favorites {
                    return respond(200, serde_json::to_value(self.settings()));
                }
                if request.expected_revision != self.revision {
                    return respond(
                        409,
                        serde_json::to_value(RevisionConflictBody::new(self.revision)),
                    );
                }
                let refused: Vec<String> = request
                    .favorites
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !self.favorites.contains(e) && !self.available(e))
                    .map(|(i, _)| format!("/favorites/{i}"))
                    .collect();
                if !refused.is_empty() {
                    return respond(422, serde_json::to_value(Self::unavailable_error(refused)));
                }
                let external: Vec<String> = request
                    .favorites
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| self.external(e))
                    .map(|(i, _)| format!("/favorites/{i}"))
                    .collect();
                if !external.is_empty() {
                    return respond(
                        422,
                        serde_json::to_value(Self::usage_error(
                            ErrorCode::EmojiUsageNotAllowed,
                            "an external emoji can only be used from an existing reaction",
                            external,
                        )),
                    );
                }
                self.favorites = request.favorites;
                self.revision += 1;
                respond(200, serde_json::to_value(self.settings()))
            }
            "getNoteReaction" => respond(
                200,
                serde_json::to_value(self.reaction_state().map_err(|e| e.to_string())?),
            ),
            "putNoteReaction" => {
                let request: PutReaction =
                    serde_json::from_value(body.cloned().ok_or("body required")?)
                        .map_err(|e| e.to_string())?;
                if let Err(details) = request.validate() {
                    let (status, value) = invalid(details);
                    return respond(status, value);
                }
                if !self.available(&request.emoji) {
                    return respond(
                        422,
                        serde_json::to_value(Self::unavailable_error(vec!["/emoji".into()])),
                    );
                }
                if self.reaction.as_ref() == Some(&request.emoji) {
                    return respond(
                        200,
                        serde_json::to_value(self.reaction_state().map_err(|e| e.to_string())?),
                    );
                }
                if self.external(&request.emoji) {
                    if !self.chips.contains(&request.emoji) {
                        return respond(
                            422,
                            serde_json::to_value(Self::usage_error(
                                ErrorCode::EmojiUsageNotAllowed,
                                "an external emoji can only be used from an existing reaction",
                                vec!["/emoji".into()],
                            )),
                        );
                    }
                    if self.reaction.is_some() {
                        return respond(
                            409,
                            serde_json::to_value(Self::usage_error(
                                ErrorCode::Conflict,
                                "remove the current reaction before reacting with an external emoji",
                                vec!["/emoji".into()],
                            )),
                        );
                    }
                }
                self.reaction = Some(request.emoji);
                respond(
                    200,
                    serde_json::to_value(self.reaction_state().map_err(|e| e.to_string())?),
                )
            }
            "deleteNoteReaction" => {
                self.reaction = None;
                respond(
                    200,
                    serde_json::to_value(self.reaction_state().map_err(|e| e.to_string())?),
                )
            }
            other => Err(format!("unknown operation {other}")),
        }
    }

    fn mutate(
        &mut self,
        id: &str,
        availability: EmojiAvailability,
        reason: Option<EmojiUnavailableReason>,
    ) {
        let entry = self.emoji.entry(id.to_owned()).or_insert(CustomEmojiSetup {
            shortcode: None,
            image_url: None,
            origin: None,
            image_state: None,
            usage: None,
            availability,
            reason,
        });
        entry.availability = availability;
        entry.reason = reason;
        if availability == EmojiAvailability::Unavailable
            && reason == Some(EmojiUnavailableReason::Deleted)
        {
            entry.image_url = None;
            if entry.image_state.is_some() {
                entry.image_state = Some(EmojiImageState::Missing);
            }
        }
    }
}

#[derive(Deserialize)]
struct Mutation {
    id: String,
    availability: EmojiAvailability,
    reason: Option<EmojiUnavailableReason>,
}

#[test]
fn scenarios_follow_the_documented_semantics_and_the_schema() -> TestResult {
    let spec = spec()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(fixture_dir().join("scenarios"))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    files.sort();
    assert!(files.len() >= 10, "scenario fixtures are missing");
    for file in files {
        let scenario: Value = serde_json::from_str(&std::fs::read_to_string(&file)?)?;
        let name = scenario["name"].as_str().ok_or("scenario name")?.to_owned();
        let mut model = Model::new(serde_json::from_value(scenario["setup"].clone())?);
        for (index, step) in scenario["steps"]
            .as_array()
            .ok_or("steps")?
            .iter()
            .enumerate()
        {
            let label = format!("{name} step {index}");
            if let Some(mutation) = step.get("mutate_emoji") {
                let m: Mutation = serde_json::from_value(mutation.clone())?;
                model.mutate(&m.id, m.availability, m.reason);
                continue;
            }
            let operation = step["operation"].as_str().ok_or("operation")?;
            let expected_status = step["expect"]["status"].as_u64().ok_or("status")?;
            let expected_body = &step["expect"]["body"];

            if let Some(body) = step.get("body") {
                let schema = schema_of(&spec, operation, &json!({"kind": "request"}))
                    .ok_or_else(|| format!("{label}: no request schema"))?;
                let valid = is_valid_against(&spec, &schema, body)?;
                // Requests the schema already forbids may still be sent to the
                // server; only validation-level 400s are exercised through
                // Rust validation instead.
                if expected_status != 400 {
                    assert!(valid, "{label}: request violates the schema");
                }
            }
            let status_key = expected_status.to_string();
            let schema = schema_of(
                &spec,
                operation,
                &json!({"kind": "response", "status": status_key}),
            )
            .ok_or_else(|| {
                format!("{label}: status {expected_status} is not documented for {operation}")
            })?;
            assert!(
                is_valid_against(&spec, &schema, expected_body)?,
                "{label}: expected body violates the schema: {expected_body}"
            );

            let (status, body) = model.handle(operation, step.get("body"))?;
            assert_eq!(u64::from(status), expected_status, "{label}: status");
            assert_eq!(&body, expected_body, "{label}: body");
            assert!(
                body.get("revision").is_none_or(|r| r.is_u64()),
                "{label}: revision"
            );
            if let Some(favorites) = body.get("favorites").and_then(Value::as_array) {
                assert!(favorites.len() <= MAX_FAVORITES, "{label}");
                for favorite in favorites {
                    let view: EmojiView = serde_json::from_value(favorite.clone())?;
                    assert_ne!(
                        view.usage,
                        Some(EmojiUsage::ExistingReactionOnly),
                        "{label}: an external emoji is never a favorite"
                    );
                }
            }
            let mut views: Vec<&Value> = body
                .get("favorites")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .collect();
            views.extend(body.get("emoji").filter(|v| v.is_object()));
            for view in views {
                let view: EmojiView = serde_json::from_value(view.clone())?;
                view.check_consistency()
                    .map_err(|problem| format!("{label}: {problem}"))?;
            }
        }
    }
    Ok(())
}

// --------------------------------------------------------------- routes ----

async fn app() -> Result<Router, Box<dyn std::error::Error>> {
    let notes = Arc::new(application::NoteService::in_memory().await);
    let key = Arc::new(driver::DevelopmentKey::generate()?);
    let federation = Arc::new(driver::Federation::new(
        "https://local.example/actor",
        vec![],
        key,
    )?);
    Ok(server::router(notes, federation))
}

const ROUTES: [(&str, &str, &str); 5] = [
    (
        "GET",
        "/api/v1/profiles/{profile}/quick-reaction-settings",
        "getQuickReactionSettings",
    ),
    (
        "PUT",
        "/api/v1/profiles/{profile}/quick-reaction-settings",
        "putQuickReactionSettings",
    ),
    (
        "GET",
        "/api/v1/profiles/{profile}/notes/{note}/reaction",
        "getNoteReaction",
    ),
    (
        "PUT",
        "/api/v1/profiles/{profile}/notes/{note}/reaction",
        "putNoteReaction",
    ),
    (
        "DELETE",
        "/api/v1/profiles/{profile}/notes/{note}/reaction",
        "deleteNoteReaction",
    ),
];

fn uri(template: &str) -> String {
    template
        .replace("{profile}", PROFILE)
        .replace("{note}", NOTE)
}

async fn assert_not_implemented(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> TestResult {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer token");
    let request = match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(body)?))?,
        None => builder.body(Body::empty())?,
    };
    let response = app.clone().oneshot(request).await?;
    assert_eq!(
        response.status(),
        StatusCode::NOT_IMPLEMENTED,
        "{method} {uri}"
    );
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = response.into_body().collect().await?.to_bytes();
    let parsed: ErrorBody = serde_json::from_slice(&bytes)?;
    assert_eq!(parsed.code, ErrorCode::NotImplemented);
    Ok(())
}

#[tokio::test]
async fn all_quick_reaction_routes_answer_an_explicit_501() -> TestResult {
    let app = app().await?;
    let spec = spec()?;
    for (method, template, operation_id) in ROUTES {
        let documented = &spec["paths"][template
            .replace("{profile}", "{profile_nanoid}")
            .replace("{note}", "{note_id}")
            .as_str()][method.to_lowercase()];
        assert_eq!(documented["operationId"], operation_id);
        let valid_body = match operation_id {
            "putQuickReactionSettings" => Some(json!({
                "expected_revision": 0,
                "favorites": [{"type": "unicode", "value": "👍"}]
            })),
            "putNoteReaction" => Some(json!({"emoji": {"type": "custom", "id": "blobcat"}})),
            _ => None,
        };
        let target = uri(template);
        assert_not_implemented(&app, method, &target, valid_body.as_ref()).await?;
        if method == "PUT" {
            // The stub never inspects the request: even an invalid body is a 501,
            // never a fabricated success or a validation error.
            let invalid = json!({"favorites": "not a list"});
            assert_not_implemented(&app, method, &target, Some(&invalid)).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_methods_are_not_part_of_the_contract() -> TestResult {
    let app = app().await?;
    for (method, path) in [
        (
            "POST",
            format!("/api/v1/profiles/{PROFILE}/quick-reaction-settings"),
        ),
        (
            "DELETE",
            format!("/api/v1/profiles/{PROFILE}/quick-reaction-settings"),
        ),
        (
            "POST",
            format!("/api/v1/profiles/{PROFILE}/notes/{NOTE}/reaction"),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&path)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn legacy_development_reaction_route_is_untouched() -> TestResult {
    // The legacy route still exists and is not served through the contract.
    let app = app().await?;
    let response = app
        .oneshot(
            Request::post(format!("/api/notes/{NOTE}/reactions"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_ne!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    Ok(())
}
