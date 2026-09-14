use nitinol::eventsource::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteId(pub Uuid);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(pub Uuid);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NoteKind {
    Post,
    Reply { target: NoteId },
    Turbo { target: NoteId },
    TurboQuote { target: NoteId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateNote {
    pub id: NoteId,
    pub author: ActorId,
    pub content: String,
    pub kind: NoteKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NoteCommand {
    Create(CreateNote),
    React { actor: ActorId, reaction: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Event)]
#[event(family = "note")]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum NoteEvent {
    NoteCreated(CreateNote),
    ReplyCreated(CreateNote),
    TurboCreated(CreateNote),
    TurboQuoteCreated(CreateNote),
    ReactionAdded {
        note: NoteId,
        actor: ActorId,
        reaction: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NoteError {
    #[error("note already exists")]
    AlreadyCreated,
    #[error("note does not exist")]
    NotCreated,
    #[error("content must not be blank")]
    EmptyContent,
    #[error("a note cannot reference itself")]
    SelfReference,
    #[error("reaction must not be blank")]
    EmptyReaction,
    #[error("turbo must not contain text; use turbo_quote")]
    TurboContent,
}
