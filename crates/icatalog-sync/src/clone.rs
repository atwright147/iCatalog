//! Create a replica directly from the server, without touching an existing file.
use crate::{
    client,
    commands::validate_base,
    download,
    protocol::PATH_CATALOGUE,
    transport::{Transport, UreqTransport},
    util, Result, SyncError,
};
use icatalog_engine::{Command, Context, Outcome, ParamSpec, Params, Result as EngineResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneReport {
    pub catalogue_id: String,
    pub generation: i64,
    pub bytes: u64,
}

fn require_absent(path: &std::path::Path) -> Result<()> {
    if path.file_name().is_none() {
        return Err(SyncError::Protocol(
            "clone requires a new catalogue filename".into(),
        ));
    }
    if !std::fs::metadata(util::parent_dir(path))?.is_dir() {
        return Err(SyncError::Io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "clone destination parent must be an existing directory",
        )));
    }
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        match std::fs::symlink_metadata(std::path::Path::new(&name)) {
            Ok(_) => return Err(SyncError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "clone destination or SQLite sidecar already exists; choose a new path or use sync for an existing replica"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Clone into a `Context::for_destination` session. Only GET requests are made.
/// Publish with an atomic no-overwrite hard link on the destination filesystem;
/// a path created by another process during download is never replaced.
pub fn clone_catalogue(
    ctx: &mut Context,
    transport: &dyn Transport,
    base: &str,
) -> Result<CloneReport> {
    validate_base(base)?;
    require_absent(ctx.path())?;
    if ctx.catalog().catalogue()?.is_some() {
        return Err(SyncError::Protocol(
            "clone requires a new destination catalogue".into(),
        ));
    }
    let info = client::info(transport, base)?;
    let url = format!("{}{}", base.trim_end_matches('/'), PATH_CATALOGUE);
    let prepared = download::prepare(transport, &url, ctx.path(), &info.id, info.generation)?;
    require_absent(ctx.path())?;
    // Unlike rename, hard_link fails atomically if any destination entry exists,
    // including a dangling symlink. The staged file is already closed and flushed.
    std::fs::hard_link(prepared.temp.path(), ctx.path()).map_err(|e| {
        SyncError::Io(std::io::Error::new(e.kind(), format!("cannot install clone at {} without overwriting: {e}; choose a new path on a local filesystem supporting hard links", ctx.path().display())))
    })?;
    util::sync_parent(ctx.path())?;
    let report = CloneReport {
        catalogue_id: info.id,
        generation: prepared.generation,
        bytes: prepared.bytes,
    };
    drop(prepared);
    *ctx = Context::open(ctx.path())?;
    Ok(report)
}

pub(crate) struct CloneCatalogue;
impl Command for CloneCatalogue {
    fn id(&self) -> &'static str {
        "clone"
    }
    fn label(&self) -> &'static str {
        "Clone a server catalogue into a new local replica"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "url",
            required: true,
            positional: false,
            doc: "base URL of the sync service, e.g. http://nas:8080; destination must not exist",
        }]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        matches!(ctx.catalog().catalogue(), Ok(None))
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
        let base = params.require("url")?.trim();
        let report = clone_catalogue(ctx, &UreqTransport::new(), base)?;
        Ok(Outcome::text(format!("cloned catalogue '{}' at generation {} into {}\ndownload: {} bytes; local replica ready for editing and sync",report.catalogue_id,report.generation,ctx.path().display(),report.bytes)))
    }
}
