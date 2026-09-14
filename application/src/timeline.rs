use std::{collections::BTreeMap, convert::Infallible, sync::Arc};

use async_trait::async_trait;
use kernel::{Note, NoteEvent, NoteId};
use nitinol::eventsource::{Aggregate, ProjectionContext, Projector};
use serde::Serialize;
use tokio::sync::{Notify, RwLock};

#[derive(Clone, Debug, Serialize)]
pub struct TimelineEntry {
    pub sequence: u64,
    pub note: Note,
}

#[derive(Clone, Default)]
pub struct Timeline {
    pub entries: Arc<RwLock<BTreeMap<NoteId, TimelineEntry>>>,
    pub updated: Arc<Notify>,
}

#[async_trait]
impl Projector<NoteEvent> for Timeline {
    type Error = Infallible;

    async fn project(
        &mut self,
        event: NoteEvent,
        ctx: &mut ProjectionContext<'_, ()>,
    ) -> Result<(), Infallible> {
        let id = match &event {
            NoteEvent::NoteCreated(created)
            | NoteEvent::ReplyCreated(created)
            | NoteEvent::TurboCreated(created)
            | NoteEvent::TurboQuoteCreated(created) => created.id,
            NoteEvent::ReactionAdded { note, .. } => *note,
        };
        let mut entries = self.entries.write().await;
        let entry = entries.entry(id).or_insert_with(|| TimelineEntry {
            sequence: ctx.current_sequence(),
            note: Note::default(),
        });
        entry.note.apply(event);
        drop(entries);
        self.updated.notify_waiters();
        Ok(())
    }
}
