//! iCatalog sync: the NAS-side HTTP service (`serve`) and the client (`sync`).
//! Revision-checked user-edit uploads and pull-only catalogues/previews (D20).
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
mod clone;
mod commands;
pub mod delta;
mod download;
mod error;
mod journal;
pub mod protocol;
pub mod security;
pub mod server;
pub mod transport;
pub mod util;

pub use clone::{clone_catalogue, CloneReport};
pub use commands::validate_base as validate_server_url;
pub use commands::{command_ids, commands, dispatch_guarded, find_command};
pub use error::{Result, SyncError};
