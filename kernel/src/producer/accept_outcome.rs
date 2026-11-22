//! The two outcomes of delivering the Accept of one Follow.
use crate::id::ActivityId;

/// The Accept answering the Follow `follow` reached the producer's inbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordAcceptDelivered {
    pub follow: ActivityId,
}

/// The Accept answering the Follow `follow` did not reach the producer's
/// inbox, for `reason`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordAcceptFailed {
    pub follow: ActivityId,
    pub reason: String,
}
