//! A Note that arrived from a producer, recorded once however often it
//! arrives.
mod aggregate;
mod event;

pub use aggregate::{Note, ReceiveNote};
pub use event::{NoteCodec, NoteEvent};
