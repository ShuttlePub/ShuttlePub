//! Checking what arrives: verifying the HTTP signature of a request to the
//! local inbox, and fetching the actor that sent it.
mod resolver;
mod verifier;

pub use verifier::SignatureVerifier;
