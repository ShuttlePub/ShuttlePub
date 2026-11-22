//! The producer aggregate: one service that asked to federate.
//!
//! Each test drives the aggregate the way the event-sourcing runtime does —
//! `decide` a command, `apply` the events it produced — and observes it
//! through the events a command produces and through the "accepted?" query.
//! Which event a command recorded is told apart by what the aggregate does
//! next: after "Accept delivered" a repeated Follow records nothing, after
//! "Accept not delivered" it records the federation again.

use std::{error::Error, fmt::Debug};

use kernel::{
    id::{ActivityId, ActorId},
    producer::{
        AcceptFederation, IsAccepted, Producer, ProducerEvent, RecordAcceptDelivered,
        RecordAcceptFailed,
    },
};
use nitinol::contract::{Aggregate, Codec, Decider, Decision, Event, Query};

type TestResult = Result<(), Box<dyn Error>>;
type ProducerCodec = <ProducerEvent as Event>::Codec;

const ACTOR: &str = "https://remote.example/actor";
const INBOX: &str = "https://remote.example/inbox";

fn follow_id(n: u32) -> Result<ActivityId, Box<dyn Error>> {
    ActivityId::parse(format!("https://remote.example/follows/{n}"))
        .map_err(|report| format!("{report:?}").into())
}

fn accept_federation(n: u32) -> Result<AcceptFederation, Box<dyn Error>> {
    Ok(AcceptFederation {
        actor: ActorId::parse(ACTOR).map_err(|report| format!("{report:?}"))?,
        inbox: INBOX.to_owned(),
        follow: follow_id(n)?,
    })
}

fn delivered(n: u32) -> Result<RecordAcceptDelivered, Box<dyn Error>> {
    Ok(RecordAcceptDelivered {
        follow: follow_id(n)?,
    })
}

fn failed(n: u32) -> Result<RecordAcceptFailed, Box<dyn Error>> {
    Ok(RecordAcceptFailed {
        follow: follow_id(n)?,
        reason: "remote inbox answered 500".to_owned(),
    })
}

/// Decides `command` against `producer`, applies what it produced, and
/// returns those events. A rejection fails the test: no command of this
/// aggregate is refused.
fn run<C>(producer: &mut Producer, command: C) -> Result<Vec<ProducerEvent>, Box<dyn Error>>
where
    Producer: Decider<C>,
    <Producer as Decider<C>>::Rejection: Debug,
{
    match producer.decide(command) {
        Decision::Accept { events, .. } => {
            for event in events.clone() {
                producer.apply(event);
            }
            Ok(events)
        }
        Decision::Reject(rejection) => Err(format!("command rejected: {rejection:?}").into()),
    }
}

fn is_accepted(producer: &Producer) -> Result<bool, Box<dyn Error>> {
    producer
        .query(IsAccepted)
        .map_err(|error| format!("{error:?}").into())
}

/// A producer that accepted Follow `n` and has not heard back about its Accept.
fn accepted(n: u32) -> Result<Producer, Box<dyn Error>> {
    let mut producer = Producer::default();
    run(&mut producer, accept_federation(n)?)?;
    Ok(producer)
}

// ------------------------------------------------------- accept federation ----

#[test]
fn producer_that_never_federated_is_not_accepted() -> TestResult {
    assert!(!is_accepted(&Producer::default())?);
    Ok(())
}

#[test]
fn accepting_federation_records_one_event_and_the_producer_is_accepted_at_once() -> TestResult {
    // Given a service that never federated.
    let mut producer = Producer::default();

    // When its federation is accepted.
    let events = run(&mut producer, accept_federation(1)?)?;

    // Then one event is recorded and the service counts as accepted before
    // any Accept has reached it.
    assert_eq!(events.len(), 1);
    assert!(is_accepted(&producer)?);
    Ok(())
}

#[test]
fn same_follow_before_its_accept_was_delivered_is_recorded_again() -> TestResult {
    let mut producer = accepted(1)?;

    let events = run(&mut producer, accept_federation(1)?)?;

    assert_eq!(events.len(), 1);
    Ok(())
}

#[test]
fn same_follow_after_its_accept_was_delivered_records_nothing() -> TestResult {
    // Given a federation whose Accept was delivered.
    let mut producer = accepted(1)?;
    run(&mut producer, delivered(1)?)?;

    // When the same Follow is accepted again.
    let events = run(&mut producer, accept_federation(1)?)?;

    // Then it succeeds without an event and the service stays accepted.
    assert!(events.is_empty());
    assert!(is_accepted(&producer)?);
    Ok(())
}

#[test]
fn same_follow_after_its_accept_failed_is_recorded_again() -> TestResult {
    // Given a federation whose Accept did not reach the service.
    let mut producer = accepted(1)?;
    run(&mut producer, failed(1)?)?;

    // When the service sends the same Follow again.
    let events = run(&mut producer, accept_federation(1)?)?;

    // Then the federation is recorded again, so its Accept goes out again.
    assert_eq!(events.len(), 1);
    assert!(is_accepted(&producer)?);
    Ok(())
}

#[test]
fn another_follow_after_the_accept_was_delivered_is_recorded() -> TestResult {
    let mut producer = accepted(1)?;
    run(&mut producer, delivered(1)?)?;

    let events = run(&mut producer, accept_federation(2)?)?;

    assert_eq!(events.len(), 1);
    Ok(())
}

// --------------------------------------------------- Accept delivered / not ----

#[test]
fn delivered_accept_is_recorded_once() -> TestResult {
    let mut producer = accepted(1)?;

    let first = run(&mut producer, delivered(1)?)?;
    let second = run(&mut producer, delivered(1)?)?;

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
    Ok(())
}

#[test]
fn failed_accept_is_recorded_while_the_accept_has_not_been_delivered() -> TestResult {
    let mut producer = accepted(1)?;

    let events = run(&mut producer, failed(1)?)?;

    assert_eq!(events.len(), 1);
    Ok(())
}

#[test]
fn failure_after_the_accept_was_delivered_records_nothing() -> TestResult {
    let mut producer = accepted(1)?;
    run(&mut producer, delivered(1)?)?;

    let events = run(&mut producer, failed(1)?)?;

    assert!(events.is_empty());
    Ok(())
}

#[test]
fn delivery_of_an_earlier_follow_does_not_count_for_the_current_one() -> TestResult {
    // Given a federation re-accepted for Follow 2 while Follow 1's Accept
    // was still on its way.
    let mut producer = accepted(1)?;
    run(&mut producer, accept_federation(2)?)?;

    // When Follow 1's Accept is reported delivered.
    let events = run(&mut producer, delivered(1)?)?;

    // Then nothing is recorded, and Follow 2 still counts as unanswered.
    assert!(events.is_empty());
    assert_eq!(run(&mut producer, accept_federation(2)?)?.len(), 1);
    Ok(())
}

// --------------------------------------------------------------- storage ----

#[test]
fn producer_events_are_stored_under_the_shuttlepub_producer_family() {
    let event_type = ProducerEvent::EVENT_TYPE;
    assert_eq!(event_type.family().as_str(), "shuttlepub.producer.v1");
    assert_eq!(event_type.type_name().as_str(), "ProducerEvent");
    assert_eq!(
        event_type.to_string(),
        "shuttlepub.producer.v1.ProducerEvent"
    );
}

#[test]
fn every_producer_event_reads_back_as_it_was_written() -> TestResult {
    let mut producer = Producer::default();
    let events = [
        run(&mut producer, accept_federation(1)?)?,
        run(&mut producer, failed(1)?)?,
        run(&mut producer, accept_federation(1)?)?,
        run(&mut producer, delivered(1)?)?,
    ]
    .concat();
    assert_eq!(events.len(), 4);
    for event in &events {
        let written = ProducerCodec::encode(event)?;
        let read = ProducerCodec::decode(&written)?;
        let rewritten = ProducerCodec::encode(&read)?;
        assert_eq!(rewritten, written);
    }
    Ok(())
}

#[test]
fn every_producer_event_is_stored_in_its_fixed_json_shape() -> TestResult {
    // The stored shape is what events already in a journal are read back
    // from, so each variant is pinned to the exact bytes it is written as.
    let cases = [
        (
            ProducerEvent::FederationAccepted {
                actor: ActorId::parse(ACTOR).map_err(|report| format!("{report:?}"))?,
                inbox: INBOX.to_owned(),
                follow: follow_id(1)?,
            },
            r#"{"type":"FederationAccepted","actor":"https://remote.example/actor","inbox":"https://remote.example/inbox","follow":"https://remote.example/follows/1"}"#,
        ),
        (
            ProducerEvent::AcceptDelivered {
                follow: follow_id(1)?,
            },
            r#"{"type":"AcceptDelivered","follow":"https://remote.example/follows/1"}"#,
        ),
        (
            ProducerEvent::AcceptFailed {
                follow: follow_id(1)?,
                reason: "remote inbox answered 500".to_owned(),
            },
            r#"{"type":"AcceptFailed","follow":"https://remote.example/follows/1","reason":"remote inbox answered 500"}"#,
        ),
    ];
    for (event, stored) in cases {
        assert_eq!(
            std::str::from_utf8(&ProducerCodec::encode(&event)?)?,
            stored
        );
        assert_eq!(ProducerCodec::decode(stored.as_bytes())?, event);
    }
    Ok(())
}
