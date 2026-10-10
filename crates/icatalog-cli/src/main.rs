//! The `icatalog` command-line interface: a thin wrapper that dispatches
//! engine commands by id. All logic lives in `icatalog-engine`.
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
            eprintln!("icatalog: internal error (panic caught); the catalogue was left unchanged");
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
            for c in icatalog_engine::commands() {
                println!("{}  {}", c.id(), c.label());
            }
            ExitCode::SUCCESS
        }
        Some(cmd) => dispatch_cli(cmd, raw.get(1..).unwrap_or(&[])),
    }
}

fn dispatch_cli(cmd: &str, rest: &[String]) -> ExitCode {
    if icatalog_engine::find_command(cmd).is_none() {
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
    match icatalog_engine::dispatch_guarded(&mut ctx, &inv.command, &inv.params) {
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
    for c in icatalog_engine::commands() {
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
