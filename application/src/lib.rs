//! Note application services.
mod codec;
mod service;
mod timeline;

pub use nitinol::eventsource::error::Retryability;
pub use service::{NoteService, ServiceError};
pub use timeline::TimelineEntry;
