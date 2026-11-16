//! The follower aggregate on its own: what `decide` answers to a Follow in each
//! state, what `apply` makes of the event, the aggregate id, and the stored form
//! of the event. No runtime and no store; these are the pure parts of the
//! aggregate that `kernel` owns (AC3).

use std::{convert::Infallible, error::Error};

use error_stack::Report;
use kernel::{
    KernelError,
    identity::{ActivityId, ActorId},
    relay::{AcceptFollow, Follower, FollowerEvent},
};
use nitinol::contract::{Aggregate, Codec, Decider, Decision, Event};

type TestResult = Result<(), Box<dyn Error>>;

const ACTOR: &str = "https://remote.example/actor";
const INBOX: &str = "https://remote.example/inbox";
const FOLLOW_ID: &str = "https://remote.example/follows/1";
const OTHER_INBOX: &str = "https://remote.example/shared-inbox";
const OTHER_FOLLOW_ID: &str = "https://remote.example/follows/2";

/// The stored payload of `followed()`. Events already in a journal are read
/// back with the current codec, so this literal must keep decoding.
const STORED_FOLLOWED: &str = r#"{"type":"Followed","actor":"https://remote.example/actor","inbox":"https://remote.example/inbox","follow":"https://remote.example/follows/1"}"#;

fn unexpected(report: Report<KernelError>) -> Box<dyn Error> {
    format!("{report:?}").into()
}

fn accept_follow(actor: &str, inbox: &str, follow: &str) -> Result<AcceptFollow, Box<dyn Error>> {
    Ok(AcceptFollow::new(
        ActorId::parse(actor).map_err(unexpected)?,
        inbox.to_owned(),
        ActivityId::parse(follow).map_err(unexpected)?,
    ))
}

fn followed() -> FollowerEvent {
    FollowerEvent::Followed {
        actor: ACTOR.to_owned(),
        inbox: INBOX.to_owned(),
        follow: FOLLOW_ID.to_owned(),
    }
}

fn following() -> Follower {
    let mut state = Follower::default();
    state.apply(followed());
    state
}

type FollowerDecision = Decision<FollowerEvent, (), Infallible>;

// ------------------------------------------------------------------ decide ----

#[test]
fn follow_from_an_actor_not_yet_following_records_one_followed_event() -> TestResult {
    // Given an actor that does not follow the local actor.
    let state = Follower::default();

    // When its Follow is accepted.
    let decision = state.decide(accept_follow(ACTOR, INBOX, FOLLOW_ID)?);

    // Then exactly one Followed event carries the actor, its inbox and the Follow id.
    let expected: FollowerDecision = Decision::Accept {
        events: vec![followed()],
        output: (),
    };
    assert_eq!(decision, expected);
    Ok(())
}

#[test]
fn follow_resent_by_a_follower_succeeds_without_an_event() -> TestResult {
    // Given an actor that already follows the local actor.
    let state = following();

    // When the same Follow is accepted again.
    let decision = state.decide(accept_follow(ACTOR, INBOX, FOLLOW_ID)?);

    // Then it is not rejected and no event is added.
    let expected: FollowerDecision = Decision::Accept {
        events: vec![],
        output: (),
    };
    assert_eq!(decision, expected);
    Ok(())
}

#[test]
fn follow_from_a_follower_with_another_inbox_and_id_adds_no_event() -> TestResult {
    // Given an actor that already follows the local actor.
    let state = following();

    // When a new Follow of it names another inbox and has another id.
    let decision = state.decide(accept_follow(ACTOR, OTHER_INBOX, OTHER_FOLLOW_ID)?);

    // Then the follower is kept as first recorded: success without an event.
    let expected: FollowerDecision = Decision::Accept {
        events: vec![],
        output: (),
    };
    assert_eq!(decision, expected);
    Ok(())
}

// ------------------------------------------------------------------- apply ----

#[test]
fn default_state_is_not_following() {
    assert!(matches!(Follower::default(), Follower::NotFollowing));
}

#[test]
fn followed_event_makes_the_actor_a_follower_with_its_inbox_and_follow() -> TestResult {
    // Given an actor that does not follow the local actor.
    let mut state = Follower::default();

    // When the Followed event is applied.
    state.apply(followed());

    // Then the state holds the actor, its inbox and the accepted Follow id.
    let Follower::Following {
        actor,
        inbox,
        follow,
    } = &state
    else {
        return Err("the state after Followed is not Following".into());
    };
    assert_eq!(actor, ACTOR);
    assert_eq!(inbox, INBOX);
    assert_eq!(follow, FOLLOW_ID);
    Ok(())
}

// ------------------------------------------------------------ aggregate id ----

#[test]
fn aggregate_id_is_the_prefixed_actor_id() -> TestResult {
    // The id is the key every stored event of the follower is filed under, so
    // changing it would orphan the followers already recorded.
    let actor = ActorId::parse(ACTOR).map_err(unexpected)?;

    assert_eq!(
        Follower::aggregate_id(&actor).as_str(),
        "shuttlepub.relay.follower:https://remote.example/actor"
    );
    Ok(())
}

#[test]
fn aggregate_id_is_the_same_for_the_same_actor_and_differs_between_actors() -> TestResult {
    let first = ActorId::parse(ACTOR).map_err(unexpected)?;
    let again = ActorId::parse(ACTOR).map_err(unexpected)?;
    let other = ActorId::parse("https://remote.example/other").map_err(unexpected)?;

    assert_eq!(
        Follower::aggregate_id(&first),
        Follower::aggregate_id(&again)
    );
    assert_ne!(
        Follower::aggregate_id(&first),
        Follower::aggregate_id(&other)
    );
    Ok(())
}

// ------------------------------------------------------------- stored form ----

#[test]
fn event_type_is_filed_under_the_relay_family() {
    let event_type = <FollowerEvent as Event>::EVENT_TYPE;

    assert_eq!(event_type.family().as_str(), "shuttlepub.relay.follower");
    assert_eq!(event_type.type_name().as_str(), "FollowerEvent");
    assert_eq!(
        event_type.to_string(),
        "shuttlepub.relay.follower.FollowerEvent"
    );
}

#[test]
fn followed_event_is_stored_as_tagged_json() -> TestResult {
    let encoded = <<FollowerEvent as Event>::Codec as Codec<FollowerEvent>>::encode(&followed())?;

    assert_eq!(std::str::from_utf8(&encoded)?, STORED_FOLLOWED);
    Ok(())
}

#[test]
fn stored_followed_event_is_read_back() -> TestResult {
    let decoded = <<FollowerEvent as Event>::Codec as Codec<FollowerEvent>>::decode(
        STORED_FOLLOWED.as_bytes(),
    )?;

    assert_eq!(decoded, followed());
    Ok(())
}
