use kernel::{ActorId, CreateNote, Note, NoteCommand, NoteError, NoteEvent, NoteId, NoteKind};
use nitinol::eventsource::{Aggregate, Decider, Decision};

fn create(kind: NoteKind, content: &str) -> CreateNote {
    CreateNote {
        id: NoteId(uuid::Uuid::new_v4()),
        author: ActorId(uuid::Uuid::new_v4()),
        content: content.into(),
        kind,
    }
}

#[test]
fn creation_events_restore_every_note_kind() {
    // Given each supported creation kind.
    let target = NoteId(uuid::Uuid::new_v4());
    for kind in [
        NoteKind::Post,
        NoteKind::Reply { target },
        NoteKind::Turbo { target },
        NoteKind::TurboQuote { target },
    ] {
        let command = create(
            kind.clone(),
            if matches!(kind, NoteKind::Turbo { .. }) {
                ""
            } else {
                "hello"
            },
        );
        let mut note = Note::default();
        // When the decision's events are applied.
        let Decision::Accept { events, .. } = note.decide(NoteCommand::Create(command.clone()))
        else {
            panic!("creation rejected")
        };
        assert_eq!(events.len(), 1);
        for event in events {
            note.apply(event);
        }
        // Then content, author, relation and identity survive.
        assert_eq!(note.created, Some(command));
    }
}

#[test]
fn duplicate_creation_is_rejected() {
    let created = create(NoteKind::Post, "hello");
    let mut note = Note::default();
    note.apply(NoteEvent::NoteCreated(created.clone()));
    let result = note.decide(NoteCommand::Create(created));
    assert_eq!(result, Decision::Reject(NoteError::AlreadyCreated));
}

#[test]
fn empty_text_and_self_references_are_rejected() {
    let note = Note::default();
    let mut command = create(NoteKind::Post, "  ");
    assert_eq!(
        note.decide(NoteCommand::Create(command.clone())),
        Decision::Reject(NoteError::EmptyContent)
    );
    command.content = "reply".into();
    command.kind = NoteKind::Reply { target: command.id };
    assert_eq!(
        note.decide(NoteCommand::Create(command)),
        Decision::Reject(NoteError::SelfReference)
    );
}

#[test]
fn reaction_is_persisted_and_duplicate_is_idempotent() {
    let mut note = Note::default();
    note.apply(NoteEvent::NoteCreated(create(NoteKind::Post, "hello")));
    let actor = ActorId(uuid::Uuid::new_v4());
    let command = NoteCommand::React {
        actor,
        reaction: "star".into(),
    };
    let Decision::Accept { events, .. } = note.decide(command.clone()) else {
        panic!("reaction rejected")
    };
    for event in events {
        note.apply(event);
    }
    assert_eq!(note.reactions.get(&actor).map(String::as_str), Some("star"));
    assert_eq!(
        note.decide(command),
        Decision::Accept {
            events: vec![],
            output: ()
        }
    );
}

#[test]
fn reaction_requires_existing_note_and_nonempty_value() {
    let mut note = Note::default();
    let actor = ActorId(uuid::Uuid::new_v4());
    assert_eq!(
        note.decide(NoteCommand::React {
            actor,
            reaction: "star".into()
        }),
        Decision::Reject(NoteError::NotCreated)
    );
    note.apply(NoteEvent::NoteCreated(create(NoteKind::Post, "hello")));
    assert_eq!(
        note.decide(NoteCommand::React {
            actor,
            reaction: " ".into()
        }),
        Decision::Reject(NoteError::EmptyReaction)
    );
}
