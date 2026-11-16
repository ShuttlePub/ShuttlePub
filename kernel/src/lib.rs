//! Domain types, the follower aggregate and the ports (traits paired with
//! `DependOn*` accessors) through which the outer layers reach external
//! systems, grouped by responsibility: `relay` for the federation inbox and
//! delivery, `identity` for the actors' identities.
pub mod error;
pub mod identity;
pub mod relay;

pub use error::KernelError;
