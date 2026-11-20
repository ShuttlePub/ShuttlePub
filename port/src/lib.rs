//! Ports whose implementations are mostly external services: delivering to
//! another server's inbox and fetching another server's actor. Each port
//! trait `Foo` is paired with `DependOnFoo`, which a composition root
//! implements to hand out its adapter. Failures are reported as one of the
//! `kernel::KernelError` classes.
pub mod delivery;
pub mod intake;
