//! Use cases, grouped like the kernel: `relay` for the federation inbox,
//! `identity` for the local actor. Each service is a trait whose dependencies
//! are its supertraits and whose behaviour lives in default methods; a blanket
//! impl gives it to every type that provides those dependencies.
pub mod config;
pub mod error;
pub mod identity;
pub mod relay;

pub use error::ApplicationError;
