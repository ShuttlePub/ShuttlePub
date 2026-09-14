use std::{collections::BTreeMap, convert::Infallible};

use nitinol::eventsource::{Aggregate, Decider, Decision, Query, Snapshotable};
use serde::{Deserialize, Serialize};

use crate::{ActorId, CreateNote, NoteCommand, NoteError, NoteEvent, NoteKind};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub created: Option<CreateNote>,
    pub reactions: BTreeMap<ActorId, String>,
}

impl Aggregate for Note {
    type Event = NoteEvent;

    fn apply(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::NoteCreated(created)
            | NoteEvent::ReplyCreated(created)
            | NoteEvent::TurboCreated(created)
            | NoteEvent::TurboQuoteCreated(created) => self.created = Some(created),
            NoteEvent::ReactionAdded {
                actor, reaction, ..
            } => {
                self.reactions.insert(actor, reaction);
            }
        }
    }
}

impl Note {
    fn creation_event(&self, command: CreateNote) -> Result<NoteEvent, NoteError> {
        if self.created.is_some() {
            return Err(NoteError::AlreadyCreated);
        }
        match command.kind {
            NoteKind::Post | NoteKind::Reply { .. } | NoteKind::TurboQuote { .. } => {
                if command.content.trim().is_empty() {
                    return Err(NoteError::EmptyContent);
                }
            }
            NoteKind::Turbo { .. } => {
                if !command.content.is_empty() {
                    return Err(NoteError::TurboContent);
                }
            }
        }
        let target = match command.kind {
            NoteKind::Post => None,
            NoteKind::Reply { target }
            | NoteKind::Turbo { target }
            | NoteKind::TurboQuote { target } => Some(target),
        };
        if target == Some(command.id) {
            return Err(NoteError::SelfReference);
        }
        Ok(match command.kind {
            NoteKind::Post => NoteEvent::NoteCreated(command),
            NoteKind::Reply { .. } => NoteEvent::ReplyCreated(command),
            NoteKind::Turbo { .. } => NoteEvent::TurboCreated(command),
            NoteKind::TurboQuote { .. } => NoteEvent::TurboQuoteCreated(command),
        })
    }
}

impl Decider<NoteCommand> for Note {
    type Output = ();
    type Rejection = NoteError;

    fn decide(&self, command: NoteCommand) -> Decision<NoteEvent, (), NoteError> {
        match command {
            NoteCommand::Create(command) => match self.creation_event(command) {
                Ok(event) => Decision::persist(vec![event]).output(()),
                Err(error) => Decision::reject(error),
            },
            NoteCommand::React { actor, reaction } => {
                let Some(created) = &self.created else {
                    return Decision::reject(NoteError::NotCreated);
                };
                if reaction.trim().is_empty() {
                    return Decision::reject(NoteError::EmptyReaction);
                }
                if self.reactions.get(&actor) == Some(&reaction) {
                    return Decision::persist(vec![]).output(());
                }
                Decision::persist(vec![NoteEvent::ReactionAdded {
                    note: created.id,
                    actor,
                    reaction,
                }])
                .output(())
            }
        }
    }
}

pub struct GetNote;

impl Query<GetNote> for Note {
    type Response = Self;
    type Error = Infallible;

    fn query(&self, _: GetNote) -> Result<Self, Infallible> {
        Ok(self.clone())
    }
}

impl Snapshotable for Note {
    type Snapshot = Self;

    fn capture(&self) -> Self {
        self.clone()
    }

    fn restore(snapshot: Self) -> Self {
        snapshot
    }
}
