//! The note aggregate: one Note that arrived from a producer. A Note is
//! recorded the first time it arrives; every later arrival succeeds without
//! adding an event, whichever activity carried it.

use std::{
    error::Error,
    fmt::Debug,
    time::{Duration, SystemTime},
};

use kernel::{
    id::{ActivityId, ActorId},
    note::{Note, NoteEvent, ReceiveNote},
};
use nitinol::contract::{Aggregate, Codec, Decider, Decision, Event};

type TestResult = Result<(), Box<dyn Error>>;
type NoteCodec = <NoteEvent as Event>::Codec;

const NOTE_ID: &str = "https://remote.example/notes/1";
const AUTHOR: &str = "https://remote.example/users/alice";

/// The command for Note `NOTE_ID` carried by Create `n`, received at a time
/// the caller chose.
fn receive(n: u32) -> Result<ReceiveNote, Box<dyn Error>> {
    let activity = format!("https://remote.example/creates/{n}");
    let raw = format!(
        r#"{{"id":"{activity}","type":"Create","actor":"{AUTHOR}","object":{{"id":"{NOTE_ID}","type":"Note","attributedTo":"{AUTHOR}"}}}}"#
    );
    Ok(ReceiveNote {
        activity: ActivityId::parse(activity).map_err(|report| format!("{report:?}"))?,
        note: ActivityId::parse(NOTE_ID).map_err(|report| format!("{report:?}"))?,
        author: ActorId::parse(AUTHOR).map_err(|report| format!("{report:?}"))?,
        producer: "remote.example".to_owned(),
        received_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000 + u64::from(n)),
        raw,
    })
}

/// Decides `command` against `note`, applies what it produced, and returns
/// those events. A rejection fails the test: an arriving Note is never refused.
fn run<C>(note: &mut Note, command: C) -> Result<Vec<NoteEvent>, Box<dyn Error>>
where
    Note: Decider<C>,
    <Note as Decider<C>>::Rejection: Debug,
{
    match note.decide(command) {
        Decision::Accept { events, .. } => {
            for event in events.clone() {
                note.apply(event);
            }
            Ok(events)
        }
        Decision::Reject(rejection) => Err(format!("command rejected: {rejection:?}").into()),
    }
}

#[test]
fn first_arrival_of_a_note_records_one_event() -> TestResult {
    let mut note = Note::default();

    let events = run(&mut note, receive(1)?)?;

    assert_eq!(events.len(), 1);
    Ok(())
}

#[test]
fn the_same_activity_arriving_again_records_nothing() -> TestResult {
    let mut note = Note::default();
    run(&mut note, receive(1)?)?;

    let events = run(&mut note, receive(1)?)?;

    assert!(events.is_empty());
    Ok(())
}

#[test]
fn the_same_note_in_another_activity_records_nothing() -> TestResult {
    let mut note = Note::default();
    run(&mut note, receive(1)?)?;

    let events = run(&mut note, receive(2)?)?;

    assert!(events.is_empty());
    Ok(())
}

#[test]
fn note_events_are_stored_under_the_shuttlepub_note_family() {
    let event_type = NoteEvent::EVENT_TYPE;
    assert_eq!(event_type.family().as_str(), "shuttlepub.note.v1");
    assert_eq!(event_type.type_name().as_str(), "NoteEvent");
    assert_eq!(event_type.to_string(), "shuttlepub.note.v1.NoteEvent");
}

#[test]
fn note_event_reads_back_as_it_was_written() -> TestResult {
    let events = run(&mut Note::default(), receive(1)?)?;
    let event = events.first().ok_or("no event recorded")?;

    let written = NoteCodec::encode(event)?;
    let read = NoteCodec::decode(&written)?;

    assert_eq!(NoteCodec::encode(&read)?, written);
    Ok(())
}

#[test]
fn note_event_is_stored_in_its_fixed_json_shape() -> TestResult {
    // The stored shape is what events already in a journal are read back
    // from, so it is pinned to the exact bytes it is written as.
    let events = run(&mut Note::default(), receive(1)?)?;
    let event = events.first().ok_or("no event recorded")?;
    let stored = concat!(
        r#"{"type":"Received","#,
        r#""activity":"https://remote.example/creates/1","#,
        r#""note":"https://remote.example/notes/1","#,
        r#""author":"https://remote.example/users/alice","#,
        r#""producer":"remote.example","#,
        r#""received_at":{"secs_since_epoch":1790000001,"nanos_since_epoch":0},"#,
        r#""raw":"{\"id\":\"https://remote.example/creates/1\",\"type\":\"Create\",\"actor\":\"https://remote.example/users/alice\",\"object\":{\"id\":\"https://remote.example/notes/1\",\"type\":\"Note\",\"attributedTo\":\"https://remote.example/users/alice\"}}"}"#,
    );

    assert_eq!(std::str::from_utf8(&NoteCodec::encode(event)?)?, stored);
    assert_eq!(&NoteCodec::decode(stored.as_bytes())?, event);
    Ok(())
}
