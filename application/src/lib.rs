//! Use cases. Each service is a trait whose dependencies are its supertraits
//! and whose behaviour lives in default methods; a blanket impl gives it to
//! every type that provides those dependencies.
pub mod config;
pub mod error;
pub mod services;
pub mod transfer;

pub use error::ApplicationError;
