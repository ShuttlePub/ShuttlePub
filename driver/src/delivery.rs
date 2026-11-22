//! Delivering to other servers: signing the request as the local actor and
//! posting the Accept to the remote inbox.
mod accept;
mod signer;

pub(crate) use signer::RequestSigner;
