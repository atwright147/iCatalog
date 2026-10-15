//! The `sync` and `serve` commands. They live here rather than in the engine
//! registry because `icatalog-sync` (L3) sits above `icatalog-engine` (L2);
//! the L4 shells merge this registry with the engine's (AGENTS section 2).

use icatalog_engine::{
    Command, Context, EngineError, Outcome, ParamSpec, Params, Result as EngineResult,
};

use crate::client::{self, SyncAction, SyncReport};
use crate::error::SyncError;
use crate::server;
use crate::transport::UreqTransport;

pub(crate) static ALL: &[&dyn Command] = &[&Sync, &Serve];

/// The sync command registry, merged with the engine's by the shells.
pub fn commands() -> &'static [&'static dyn Command] {
    ALL
}

/// Find a sync command by id.
pub fn find_command(id: &str) -> Option<&'static dyn Command> {
    ALL.iter().copied().find(|c| c.id() == id)
}

/// Ids of all sync commands.
pub fn command_ids() -> Vec<&'static str> {
    ALL.iter().map(|c| c.id()).collect()
}

/// Dispatch a sync command, converting a panic into an error (the last-resort
/// safety net, mirroring `icatalog_engine::dispatch_guarded`).
pub fn dispatch_guarded(ctx: &mut Context, id: &str, params: &Params) -> EngineResult<Outcome> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let cmd = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if !cmd.enabled(ctx) {
            return Err(EngineError::Unsupported(format!(
                "command '{id}' is not available for this catalogue"
            )));
        }
        cmd.run(ctx, params)
    }))
    .unwrap_or_else(|_| {
        Err(EngineError::Internal(format!(
            "command '{id}' panicked; catalogue left unchanged"
        )))
    })
}

fn is_initialised(ctx: &Context) -> bool {
    matches!(ctx.catalog().catalogue(), Ok(Some(_)))
}

struct Sync;

impl Command for Sync {
    fn id(&self) -> &'static str {
        "sync"
    }
    fn label(&self) -> &'static str {
        "Sync this catalogue with the NAS"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "url",
            required: true,
            positional: false,
            doc: "base URL of the sync service, e.g. http://nas:8080",
        }]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
        let base = params.require("url")?.trim().to_owned();
        validate_base(&base)?;
        let transport = UreqTransport::new();
        match client::sync_once(ctx, &transport, &base) {
            Ok(report) => Ok(render(&report)),
            // Offline is deferred and reported, never fatal (docs/sync.md section 1).
            Err(SyncError::Unreachable(m)) => Ok(Outcome::text(format!("sync deferred: {m}"))),
            Err(e) => Err(e.into()),
        }
    }
}

struct Serve;

impl Command for Serve {
    fn id(&self) -> &'static str {
        "serve"
    }
    fn label(&self) -> &'static str {
        "Serve this catalogue for sync (NAS side)"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "port",
                required: true,
                positional: false,
                doc: "TCP port to listen on",
            },
            ParamSpec {
                name: "bind",
                required: false,
                positional: false,
                doc: "address to bind (default 127.0.0.1)",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
        let port = match params.get_i64("port")? {
            Some(p) if (1..=65535).contains(&p) => p,
            Some(p) => {
                return Err(EngineError::BadParam(format!(
                    "port must be 1-65535 (got {p})"
                )))
            }
            None => {
                return Err(EngineError::BadParam(
                    "missing required parameter 'port'".into(),
                ))
            }
        };
        let bind = params.get("bind").unwrap_or("127.0.0.1").trim();
        if bind.is_empty() {
            return Err(EngineError::BadParam("bind must not be empty".into()));
        }
        let addr = format!("{bind}:{port}");
        let server = server::bind(&addr)?;
        println!("icatalog sync service listening on {addr}");
        server::serve_loop(server, ctx)?;
        Ok(Outcome::text("sync service stopped"))
    }
}

fn validate_base(base: &str) -> EngineResult<()> {
    if base.is_empty() {
        return Err(EngineError::BadParam("url must not be empty".into()));
    }
    if base.starts_with("https://") {
        return Err(EngineError::Unsupported(
            "https is not supported in this slice; use http on a trusted LAN (docs/sync.md)".into(),
        ));
    }
    if !base.starts_with("http://") {
        return Err(EngineError::BadParam(format!(
            "url must start with http:// (got '{base}')"
        )));
    }
    if base.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(EngineError::BadParam(
            "url must not contain whitespace or control characters".into(),
        ));
    }
    Ok(())
}

fn render(r: &SyncReport) -> Outcome {
    let mut lines = Vec::new();
    match &r.action {
        SyncAction::UpToDate => lines.push(format!(
            "already up to date at generation {}; nothing to push",
            r.local_generation
        )),
        SyncAction::Pushed => lines.push(format!(
            "push: accepted; server generation is now {}",
            r.server_generation
        )),
        SyncAction::Pulled { backup } => {
            lines.push(format!(
                "pull: local copy updated to server generation {}",
                r.server_generation
            ));
            if let Some(b) = backup {
                lines.push(format!("backup: {} (previous local copy)", b.display()));
            }
        }
        SyncAction::Resynced { backup } => {
            lines.push(format!(
                "push refused: the server generation moved to {}; local copy resynced",
                r.server_generation
            ));
            if let Some(b) = backup {
                lines.push(format!(
                    "backup: {} (your local edits before the resync)",
                    b.display()
                ));
            }
        }
    }
    if r.bytes > 0 {
        lines.push(format!("snapshot: {} bytes", r.bytes));
    }
    Outcome::text(lines.join("\n"))
}
