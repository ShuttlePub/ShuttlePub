//! Use cases and the reactions to recorded events. Each service is a trait
//! whose dependencies are its supertraits and whose behaviour lives in default
//! methods; a blanket impl gives it to every type that provides those
//! dependencies.
//!
//! Modules follow the domain's groupings: `community` (this service itself),
//! `intake` (checking what arrives and turning it into events), `producer` and
//! `note` (the two aggregates), `delivery` (answering a federation with an
//! Accept). `eventsource` hands out the event-sourcing system they run on.
pub mod community;
pub mod config;
pub mod delivery;
pub mod error;
pub mod eventsource;
pub mod intake;
pub mod note;
pub mod producer;

pub use error::ApplicationError;
