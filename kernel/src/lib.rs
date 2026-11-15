//! Domain entities, value objects and the ports (traits paired with
//! `DependOn*` accessors) through which the outer layers reach external systems.
pub mod entities;
pub mod error;
pub mod interface;

pub use error::KernelError;
