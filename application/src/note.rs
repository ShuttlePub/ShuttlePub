use error_stack::Report;
use kernel::note::{Note, ReceiveNote};
use nitinol::eventsource::{AskError, RetryPolicy};

use crate::{ApplicationError, eventsource::DependOnEventSourceSystem};

/// The Note aggregates: one per Note that arrived.
pub trait NoteService: DependOnEventSourceSystem {
    /// Records the Note unless it is already known; either way it succeeds.
    fn receive(
        &self,
        command: ReceiveNote,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            let outcome = self
                .event_source_system()
                .aggregate_proxy::<Note>(Note::aggregate_id(&command.note))
                .ask_with_retry(command, &RetryPolicy::default())
                .await;
            match outcome {
                // Another activation wrote this Note's first event first: the
                // Note is known, which is all receiving it again asks for.
                Ok(()) | Err(AskError::AlreadyCreated) => Ok(()),
                Err(error) => Err(Report::new(error).change_context(ApplicationError::Internal)),
            }
        }
    }
}

impl<T: DependOnEventSourceSystem> NoteService for T {}

pub trait DependOnNoteService: Send + Sync + 'static {
    type NoteService: NoteService;
    fn note_service(&self) -> &Self::NoteService;
}
