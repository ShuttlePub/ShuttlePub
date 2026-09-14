//! Note domain contracts.
mod note;
mod types;

pub use note::{GetNote, Note};
pub use types::{ActorId, CreateNote, NoteCommand, NoteError, NoteEvent, NoteId, NoteKind};
