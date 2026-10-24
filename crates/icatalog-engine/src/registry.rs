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
    run_atomic(ctx, id, |ctx| cmd.run(ctx, params))
}

/// Engine commands commit as a unit, including writes performed before an
/// error or panic. Sync uses a separate dispatcher and file-replacement policy.
fn run_atomic(
    ctx: &mut Context,
    id: &str,
    run: impl FnOnce(&mut Context) -> Result<Outcome>,
) -> Result<Outcome> {
    use icatalog_catalog::CatalogError;
    ctx.catalog()
        .connection()
        .execute_batch("BEGIN DEFERRED")
        .map_err(CatalogError::from)?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(ctx)))
        .unwrap_or_else(|_| Err(EngineError::Internal(format!("command '{id}' panicked"))));
    let result = result.and_then(|outcome| {
        ctx.catalog()
            .connection()
            .execute_batch("COMMIT")
            .map_err(CatalogError::from)?;
        Ok(outcome)
    });
    if let Err(error) = result {
        // SQLite can already have rolled back after a disk/I/O failure.
        if !ctx.catalog().connection().is_autocommit() {
            if let Err(rollback) = ctx.catalog().connection().execute_batch("ROLLBACK") {
                return Err(EngineError::Internal(format!(
                    "{error}; rollback failed: {rollback}; close and reopen the catalogue before retrying"
                )));
            }
        }
        return Err(error);
    }
    result
}

/// Last-resort guard for dispatch, in addition to the command transaction.
/// Failures outside command execution make no claim about earlier commands.
pub fn dispatch_guarded(ctx: &mut Context, id: &str, params: &Params) -> Result<Outcome> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(ctx, id, params)))
        .unwrap_or_else(|_| Err(EngineError::Internal(format!("command '{id}' panicked"))))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn panic_after_write_rolls_back_and_session_remains_usable() {
        let mut ctx = Context::open(":memory:").unwrap();
        let result = run_atomic(&mut ctx, "synthetic", |ctx| {
            ctx.catalog().insert_catalogue("rolled-back", "t")?;
            panic!("synthetic failure after a write");
        });
        assert!(result.is_err());
        assert!(ctx.catalog().catalogue().unwrap().is_none());
        assert!(ctx.catalog().connection().is_autocommit());
        dispatch_guarded(&mut ctx, "init", &Params::new()).unwrap();
        assert!(ctx.catalog().catalogue().unwrap().is_some());
    }

    #[test]
    fn failed_commit_rolls_back() {
        let mut ctx = Context::open(":memory:").unwrap();
        let result = run_atomic(&mut ctx, "synthetic", |ctx| {
            ctx.catalog().connection().execute_batch(
                "PRAGMA defer_foreign_keys = ON; INSERT INTO folder (parent_id, name) VALUES (999, 'orphan');"
            ).map_err(icatalog_catalog::CatalogError::from)?;
            Ok(Outcome::text("must not commit"))
        });
        assert!(result.is_err());
        assert!(ctx.catalog().all_folders().unwrap().is_empty());
        assert!(ctx.catalog().connection().is_autocommit());
    }
}
