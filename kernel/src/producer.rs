//! A service that asked to federate with this one, and through which Notes
//! arrive: the Follow it sent, the Accept that answers it, and the producer
//! aggregate that records the federation and whether its Accept arrived.
mod accept;
mod accept_outcome;
mod aggregate;
mod event;
mod federation;
mod follow;

pub use accept::Accept;
pub use accept_outcome::{RecordAcceptDelivered, RecordAcceptFailed};
pub use aggregate::{IsAccepted, Producer};
pub use event::{ProducerCodec, ProducerEvent};
pub use federation::AcceptFederation;
pub use follow::Follow;
