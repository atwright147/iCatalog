use crate::command::{Command, Outcome};
use crate::context::Context;
use crate::error::{EngineError, Result};
use crate::params::Params;

/// All registered commands.
pub fn commands() -> &'static [&'static dyn Command] {
    crate::commands::ALL
}

/// Find a command by id.
pub fn find_command(id: &str) -> Option<&'static dyn Command> {
    commands().iter().copied().find(|c| c.id() == id)
}

/// Ids of all commands.
pub fn command_ids() -> Vec<&'static str> {
    commands().iter().map(|c| c.id()).collect()
}

/// Run a command by id.
pub fn dispatch(ctx: &mut Context, id: &str, params: &Params) -> Result<Outcome> {
    let cmd = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
    if !cmd.enabled(ctx) {
        return Err(EngineError::Unsupported(format!(
            "command '{id}' is not available for this catalogue"
        )));
    }
    cmd.run(ctx, params)
}

/// Like [`dispatch`], but converts a panic into an error. This is the
/// last-resort safety net (AGENTS golden rule), not a licence to panic.
pub fn dispatch_guarded(ctx: &mut Context, id: &str, params: &Params) -> Result<Outcome> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(ctx, id, params)))
        .unwrap_or_else(|_| {
            Err(EngineError::Internal(format!(
                "command '{id}' panicked; catalogue left unchanged"
            )))
        })
}
