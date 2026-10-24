//! The `icatalog` command-line interface: a thin wrapper that dispatches
//! commands by id, from the engine registry and the sync registry.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod args;

use std::process::ExitCode;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&raw))) {
        Ok(code) => code,
        Err(_) => {
            eprintln!(
                "icatalog: internal error (panic caught); reopen the catalogue to check its state"
            );
            ExitCode::from(70)
        }
    }
}

fn run(raw: &[String]) -> ExitCode {
    match raw.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => {
            println!("{}", help_text());
            ExitCode::SUCCESS
        }
        Some("--version") | Some("-V") => {
            println!("icatalog {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("commands") => {
            for c in all_commands() {
                println!("{}  {}", c.id(), c.label());
            }
            ExitCode::SUCCESS
        }
        Some(cmd) => dispatch_cli(cmd, raw.get(1..).unwrap_or(&[])),
    }
}

/// The engine registry plus the sync registry (`icatalog-sync` is L3, above the
/// engine, so it cannot join the engine's `ALL`; the L4 shells merge them).
fn all_commands() -> Vec<&'static dyn icatalog_engine::Command> {
    let mut v: Vec<&'static dyn icatalog_engine::Command> = icatalog_engine::commands().to_vec();
    v.extend(icatalog_sync::commands().iter().copied());
    v
}

fn is_known(cmd: &str) -> bool {
    icatalog_engine::find_command(cmd).is_some() || icatalog_sync::find_command(cmd).is_some()
}

fn dispatch_any(
    ctx: &mut icatalog_engine::Context,
    id: &str,
    params: &icatalog_engine::Params,
) -> icatalog_engine::Result<icatalog_engine::Outcome> {
    if icatalog_engine::find_command(id).is_some() {
        icatalog_engine::dispatch_guarded(ctx, id, params)
    } else {
        icatalog_sync::dispatch_guarded(ctx, id, params)
    }
}

fn dispatch_cli(cmd: &str, rest: &[String]) -> ExitCode {
    if !is_known(cmd) {
        eprintln!("unknown command '{cmd}'");
        eprintln!("{}", help_text());
        return ExitCode::from(2);
    }
    let inv = match args::parse(cmd, rest) {
        Ok(i) => i,
        Err(msg) => {
            eprintln!("icatalog: {msg}\n");
            eprintln!("{}", help_text());
            return ExitCode::from(2);
        }
    };
    let mut ctx = match icatalog_engine::Context::open(&inv.catalogue) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("icatalog: {e}");
            return ExitCode::FAILURE;
        }
    };
    match dispatch_any(&mut ctx, &inv.command, &inv.params) {
        Ok(o) => {
            println!("{}", o.render());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("icatalog: {e}");
            ExitCode::FAILURE
        }
    }
}

fn help_text() -> String {
    let mut out = String::from("Usage: icatalog <command> <catalogue> [options]\n\n");
    out.push_str("Commands:\n");
    for c in all_commands() {
        out.push_str(&format!("  {}  {}\n", c.id(), c.label()));
        for p in c.params() {
            let req = if p.required { " (required)" } else { "" };
            let shown = if p.positional {
                format!("<{}>", p.name)
            } else {
                format!("--{} <value>", p.name)
            };
            out.push_str(&format!("      {shown}{req}  {}\n", p.doc));
        }
    }
    out.push_str("\nGlobal:\n");
    out.push_str("  --help      show this help\n");
    out.push_str("  --version   show the version\n");
    out.push_str("  commands    list registered commands\n");
    out
}
