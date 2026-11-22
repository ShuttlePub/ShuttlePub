//! Helpers shared by the server's integration-test binaries: a remote
//! ActivityPub service on a loopback port, the way it signs requests to the
//! local inbox, the activities it sends, the server binary as a child process,
//! and the PostgreSQL the application, in-process or as the binary, runs on.
#![allow(
    dead_code,
    reason = "every test binary compiles all helpers but uses a different subset of them"
)]

pub mod activity;
pub mod binary;
pub mod peer;
pub mod postgres;
pub mod signing;
pub mod wait;
