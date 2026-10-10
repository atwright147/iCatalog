//! L2 session and command registry for iCatalog. Every user-visible action
//! is a command; the CLI, GUI and sync all dispatch by id.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod command;
mod commands;
mod context;
mod error;
mod params;
mod registry;
mod time;

pub use command::{Command, Outcome, ParamSpec};
pub use context::Context;
pub use error::{EngineError, Result};
pub use params::Params;
pub use registry::{command_ids, commands, dispatch, dispatch_guarded, find_command};
