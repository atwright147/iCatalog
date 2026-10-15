//! iCatalog sync: the NAS-side HTTP service (`serve`) and the client (`sync`).
//! Whole-snapshot transfer with a generation guard; see `docs/sync.md`.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod client;
mod commands;
mod error;
pub mod protocol;
pub mod server;
pub mod transport;
pub mod util;

pub use commands::{command_ids, commands, dispatch_guarded, find_command};
pub use error::{Result, SyncError};
