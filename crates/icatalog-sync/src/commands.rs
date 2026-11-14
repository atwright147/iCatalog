//! The `sync` and `serve` commands. They live here rather than in the engine
//! registry because `icatalog-sync` (L3) sits above `icatalog-engine` (L2);
//! the L4 shells merge this registry with the engine's (AGENTS section 2).

use icatalog_engine::{
    Command, Context, EngineError, Outcome, ParamSpec, Params, Result as EngineResult,
    SyncCheckResult, SyncCheckStatus, SyncChoice, SyncConflict, SyncEntity, SyncEntityKind,
    SyncFieldConflict, SyncOutcome, SyncPhase, SyncProgress, SyncResolveResult, SyncSharedField,
    SyncValue,
};

use crate::client::{self, SyncAction, SyncReport};
use crate::error::SyncError;
use crate::server;
use crate::transport::UreqTransport;

pub(crate) static ALL: &[&dyn Command] = &[
    &Sync,
    &SyncCheck,
    &SyncResolve,
    &Serve,
    &crate::clone::CloneCatalogue,
];

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
            "command '{id}' panicked; reopen the catalogue and check sync status"
        )))
    })
}

fn is_initialised(ctx: &Context) -> bool {
    matches!(ctx.catalog().catalogue(), Ok(Some(_)))
}

struct Sync;

struct SyncCheck;
impl Command for SyncCheck {
    fn id(&self) -> &'static str {
        "sync-check"
    }
    fn label(&self) -> &'static str {
        "Check whether this catalogue needs sync"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "url",
                required: true,
                positional: false,
                doc: "base URL of the sync service",
            },
            crate::security::TOKEN_FILE,
            crate::security::CA_FILE,
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
        let base = params.require("url")?.trim().to_owned();
        validate_base(&base)?;
        let transport = UreqTransport::from_params(params, &base)?;
        ctx.report_progress(SyncProgress {
            phase: SyncPhase::Checking,
            completed: None,
            total: None,
        });
        let latched_required = ctx.sync_required()?;
        let (row, replica, baseline, base_generation, changes) = {
            let snapshot = ctx
                .catalog()
                .connection()
                .unchecked_transaction()
                .map_err(icatalog_catalog::CatalogError::from)?;
            let row = ctx
                .catalog()
                .catalogue()?
                .ok_or_else(|| EngineError::Unsupported("catalogue is not initialised".into()))?;
            let replica = ctx.catalog().is_replica()?;
            let baseline = ctx.catalog().replica_base()?;
            let (base_generation, changes) =
                crate::delta::pending(&row.id, baseline.as_ref(), ctx.catalog())?;
            snapshot
                .commit()
                .map_err(icatalog_catalog::CatalogError::from)?;
            (row, replica, baseline, base_generation, changes)
        };
        if !replica {
            return Ok(Outcome::Sync(SyncOutcome::Check(SyncCheckResult {
                status: SyncCheckStatus::Untracked,
                catalogue_id: row.id.clone(),
                baseline_generation: None,
                local_generation: Some(row.generation),
                server_generation: None,
                replica,
                sync_required: latched_required,
                pending: Vec::new(),
                conflicts: Vec::new(),
                recovery_backup: crate::journal::recovery_backup(ctx)?,
            })));
        }
        let remote = match client::info(&transport, &base) {
            Ok(info) => info,
            Err(SyncError::Unreachable(reason)) => {
                ctx.report_progress(SyncProgress {
                    phase: SyncPhase::Deferred,
                    completed: None,
                    total: None,
                });
                return Ok(Outcome::Sync(SyncOutcome::Check(SyncCheckResult {
                    status: SyncCheckStatus::Unreachable(reason),
                    catalogue_id: row.id.clone(),
                    baseline_generation: baseline.as_ref().map(|(generation, _)| *generation),
                    local_generation: Some(row.generation),
                    server_generation: None,
                    replica,
                    sync_required: latched_required,
                    pending: changes.iter().map(pending_entity).collect(),
                    conflicts: journal_conflicts(ctx)?,
                    recovery_backup: crate::journal::recovery_backup(ctx)?,
                })));
            }
            Err(error) => return Err(error.into()),
        };
        if remote.id != row.id {
            return Err(EngineError::Internal(
                "server catalogue id does not match the local catalogue".into(),
            ));
        }
        if baseline.is_some() && remote.generation < base_generation {
            return Err(SyncError::Protocol(
                "server generation moved backwards; refusing to treat the replica as current"
                    .into(),
            )
            .into());
        }
        let required = latched_required
            || baseline.is_none()
            || !changes.is_empty()
            || remote.generation != base_generation;
        if required {
            ctx.set_sync_required(true)?;
        }
        ctx.report_progress(SyncProgress {
            phase: SyncPhase::Complete,
            completed: None,
            total: None,
        });
        Ok(Outcome::Sync(SyncOutcome::Check(SyncCheckResult {
            status: SyncCheckStatus::Reachable,
            catalogue_id: row.id.clone(),
            baseline_generation: baseline.as_ref().map(|(generation, _)| *generation),
            local_generation: Some(row.generation),
            server_generation: Some(remote.generation),
            replica,
            sync_required: required,
            pending: changes.iter().map(pending_entity).collect(),
            conflicts: journal_conflicts(ctx)?,
            recovery_backup: crate::journal::recovery_backup(ctx)?,
        })))
    }
}

struct SyncResolve;
impl Command for SyncResolve {
    fn id(&self) -> &'static str {
        "sync-resolve"
    }
    fn label(&self) -> &'static str {
        "Inspect and resolve sync conflicts"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "action",
                required: true,
                positional: false,
                doc: "list, show, choose, apply or recover",
            },
            ParamSpec {
                name: "key",
                required: false,
                positional: false,
                doc: "conflict key such as asset:<uid>",
            },
            ParamSpec {
                name: "field",
                required: false,
                positional: false,
                doc: "conflicting field name",
            },
            ParamSpec {
                name: "choice",
                required: false,
                positional: false,
                doc: "local, server or combined (sets only)",
            },
            ParamSpec {
                name: "session-id",
                required: false,
                positional: false,
                doc: "active conflict session identifier returned by list",
            },
            ParamSpec {
                name: "page",
                required: false,
                positional: false,
                doc: "zero-based conflict-list page (default 0)",
            },
            ParamSpec {
                name: "page-size",
                required: false,
                positional: false,
                doc: "conflicts per page, 1 through 200 (default 50)",
            },
            ParamSpec {
                name: "url",
                required: false,
                positional: false,
                doc: "server URL, required by apply",
            },
            crate::security::TOKEN_FILE,
            crate::security::CA_FILE,
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
        match params.require("action")? {
            "list" => {
                if let Some(supplied) = params.get("session-id") {
                    crate::journal::validate_session(ctx, supplied)?;
                }
                let page = parse_page(params.get("page"), 0)?;
                let page_size = parse_page(params.get("page-size"), 50)?;
                if page_size == 0 || page_size > 200 {
                    return Err(EngineError::BadParam(
                        "page-size must be between 1 and 200".into(),
                    ));
                }
                let mut all = journal_conflicts(ctx)?;
                let total = all.len();
                let start = page
                    .checked_mul(page_size)
                    .ok_or_else(|| EngineError::BadParam("page offset is too large".into()))?;
                let conflicts = if start >= total {
                    Vec::new()
                } else {
                    all.drain(start..total.min(start.saturating_add(page_size)))
                        .collect()
                };
                Ok(Outcome::Sync(SyncOutcome::Resolve(resolve_result(
                    ctx, conflicts, false, page, page_size, total,
                )?)))
            }
            "show" => {
                validate_action_session(ctx, params)?;
                let key = params.require("key")?;
                let mut conflicts = journal_conflicts(ctx)?;
                conflicts.retain(|item| item.key == key);
                if conflicts.is_empty() {
                    return Err(EngineError::BadParam("unknown sync conflict key".into()));
                }
                let total = conflicts.len();
                Ok(Outcome::Sync(SyncOutcome::Resolve(resolve_result(
                    ctx, conflicts, false, 0, 1, total,
                )?)))
            }
            "choose" => {
                validate_action_session(ctx, params)?;
                if matches!(
                    crate::journal::recovery_phase(ctx)?.as_deref(),
                    Some("applying" | "applied")
                ) {
                    return Err(EngineError::Unsupported("a prepared conflict resolution is pending; retry apply before changing any choices".into()));
                }
                let key = params.require("key")?;
                let field = params.require("field")?;
                let choice = params.require("choice")?;
                let valid_choice = matches!(choice, "local" | "server" | "combined");
                if !valid_choice {
                    return Err(EngineError::BadParam(
                        "choice must be local, server or combined".into(),
                    ));
                }
                let conflicts = journal_conflicts(ctx)?;
                let item = conflicts
                    .iter()
                    .find(|item| item.key == key)
                    .ok_or_else(|| EngineError::BadParam("unknown sync conflict key".into()))?;
                let value = item
                    .fields
                    .iter()
                    .find(|candidate| candidate.field == field)
                    .ok_or_else(|| {
                        EngineError::BadParam("field is not conflicted for this entity".into())
                    })?;
                if choice == "combined" && !matches!(value.local, SyncValue::Set(_)) {
                    return Err(EngineError::BadParam(
                        "combined is available only for keyword or manual membership sets".into(),
                    ));
                }
                crate::journal::choose(ctx, key, field, choice)?;
                let conflicts: Vec<_> = journal_conflicts(ctx)?
                    .into_iter()
                    .filter(|item| item.key == key)
                    .collect();
                let total = conflicts.len();
                Ok(Outcome::Sync(SyncOutcome::Resolve(resolve_result(
                    ctx, conflicts, false, 0, 1, total,
                )?)))
            }
            "apply" => {
                validate_action_session(ctx, params)?;
                apply_conflict_choices(ctx, params)
            }
            "recover" => {
                if crate::journal::recovery_phase(ctx)?.as_deref() != Some("snapshot_ready") {
                    return Err(EngineError::Unsupported(
                        "there is no verified staged snapshot awaiting recovery; retry sync or apply the prepared conflict resolution".into(),
                    ));
                }
                let recovered = client::recover_staged_snapshot(ctx)?;
                let mut all = journal_conflicts(ctx)?;
                let total = all.len();
                let page_size = 50;
                all.truncate(page_size);
                let mut result = resolve_result(ctx, all, false, 0, page_size, total)?;
                result.recovered = recovered;
                Ok(Outcome::Sync(SyncOutcome::Resolve(result)))
            }
            _ => Err(EngineError::BadParam(
                "action must be list, show, choose, apply or recover".into(),
            )),
        }
    }
}

fn validate_action_session(ctx: &Context, params: &Params) -> EngineResult<()> {
    let supplied = params.require("session-id")?;
    crate::journal::validate_session(ctx, supplied)?;
    Ok(())
}

fn parse_page(raw: Option<&str>, default: usize) -> EngineResult<usize> {
    match raw {
        None => Ok(default),
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| EngineError::BadParam("page values must be non-negative integers".into())),
    }
}

fn resolve_result(
    ctx: &Context,
    conflicts: Vec<SyncConflict>,
    applied: bool,
    page: usize,
    page_size: usize,
    total_conflicts: usize,
) -> EngineResult<SyncResolveResult> {
    let all_choices_made = journal_conflicts(ctx)?
        .iter()
        .all(|conflict| conflict.fields.iter().all(|field| field.choice.is_some()));
    Ok(SyncResolveResult {
        conflicts,
        applied,
        recovery_backup: crate::journal::recovery_backup(ctx)?,
        session_id: crate::journal::session_id(ctx)?,
        total_conflicts,
        page,
        page_size,
        all_choices_made,
        recovery_phase: crate::journal::recovery_phase(ctx)?,
        recovery_pending: matches!(
            crate::journal::recovery_phase(ctx)?.as_deref(),
            Some("snapshot_ready" | "retry" | "applying" | "applied")
        ),
        recovered: false,
    })
}

fn apply_conflict_choices(ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
    client::recover_staged_snapshot(ctx)?;
    if crate::journal::recovery_phase(ctx)?.as_deref() == Some("retry") {
        return Err(EngineError::Unsupported("the prior sync stopped before replacing the local catalogue; run sync again before resolving conflicts".into()));
    }
    let base = params.require("url")?.trim().to_owned();
    validate_base(&base)?;
    let transport = UreqTransport::from_params(params, &base)?;
    let stored = crate::journal::conflicts(ctx)?;
    if stored.is_empty() {
        return Err(EngineError::Unsupported(
            "there are no unresolved sync conflicts".into(),
        ));
    }
    let resuming = crate::journal::recovery_phase(ctx)?.as_deref() == Some("applying");
    let merged = if resuming {
        crate::journal::resolution_records(ctx)?
    } else {
        let mut merged = Vec::new();
        for item in &stored {
            let mut value = item.server.clone();
            let changed_fields = journal_conflicts(ctx)?
                .into_iter()
                .find(|c| c.key == item.key)
                .map(|c| c.fields)
                .unwrap_or_default();
            for conflict in changed_fields {
                let choice = item
                    .choices
                    .iter()
                    .find(|(field, _)| field == &conflict.field)
                    .map(|(_, choice)| choice.as_str())
                    .ok_or_else(|| {
                        EngineError::Unsupported(format!(
                            "choose a value for {} in {} before applying",
                            conflict.field, item.key
                        ))
                    })?;
                apply_field_choice(&mut value, &item.local, &conflict.field, choice)?;
            }
            merged.push((item.key.clone(), value));
        }
        merged
    };
    // Reject stale editor state before recording any catalogue writes. A crash
    // during apply can be retried from the prepared merged records in the journal.
    let active: std::collections::BTreeMap<_, _> = crate::delta::current(ctx.catalog())?
        .into_iter()
        .map(|v| (v.key(), v))
        .collect();
    for item in &stored {
        let expected = active.get(&item.key).ok_or_else(|| {
            EngineError::Unsupported(
                "catalogue changed since the conflict was reviewed; sync again before resolving"
                    .into(),
            )
        })?;
        let prepared = merged
            .iter()
            .find(|(key, _)| key == &item.key)
            .map(|(_, value)| value)
            .ok_or_else(|| EngineError::Internal("missing prepared conflict resolution".into()))?;
        if !expected.same_value(&item.server) && !expected.same_value(prepared) {
            return Err(EngineError::Unsupported(
                "catalogue changed since the conflict was reviewed; sync again before resolving"
                    .into(),
            ));
        }
    }
    if !resuming {
        let row = ctx
            .catalog()
            .catalogue()?
            .ok_or_else(|| EngineError::Unsupported("catalogue is not initialised".into()))?;
        crate::journal::prepare_resolution(ctx, &row.id, &merged)?;
    }
    // Validate every choice and destination before the first write, then apply
    // all conflicts atomically. A restart before sync can safely retry this.
    ctx.catalog()
        .connection()
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(icatalog_catalog::CatalogError::from)?;
    let apply_result = (|| -> EngineResult<()> {
        let now = icatalog_engine::utc_now_iso8601();
        for (_, change) in &merged {
            let current = crate::delta::current(ctx.catalog())?
                .into_iter()
                .find(|v| v.key() == change.key());
            if current.as_ref().is_some_and(|v| v.same_value(change)) {
                continue;
            }
            match &change.value {
                crate::delta::Value::Annotation(state) => {
                    ctx.catalog().apply_annotation_state(state, &now)?
                }
                crate::delta::Value::Collection(state) => {
                    ctx.catalog().apply_collection_state(state)?
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = apply_result {
        let _ = ctx.catalog().connection().execute_batch("ROLLBACK");
        return Err(error);
    }
    if let Err(error) = ctx.catalog().connection().execute_batch("COMMIT") {
        let rollback = ctx.catalog().connection().execute_batch("ROLLBACK");
        if rollback.is_err() {
            return Err(EngineError::Internal(format!(
                "conflict application commit failed and rollback also failed: {error}"
            )));
        }
        return Err(icatalog_catalog::CatalogError::from(error).into());
    }
    crate::journal::set_phase(ctx, "applied")?;
    match client::sync_once_with_requirement(ctx, &transport, &base, true) {
        Ok(report) => {
            if report.conflicts.is_empty() {
                crate::journal::clear_conflicts(ctx)?;
                ctx.set_sync_required(false)?;
            }
            let mut conflicts = journal_conflicts(ctx)?;
            let total = conflicts.len();
            let page_size = 50;
            conflicts.truncate(page_size);
            Ok(Outcome::Sync(SyncOutcome::Resolve(resolve_result(
                ctx,
                conflicts,
                report.conflicts.is_empty(),
                0,
                page_size,
                total,
            )?)))
        }
        Err(error) => Err(error.into()),
    }
}

fn apply_field_choice(
    target: &mut crate::delta::Change,
    local: &crate::delta::Change,
    field: &str,
    choice: &str,
) -> EngineResult<()> {
    match (&mut target.value, &local.value, field, choice) {
        (
            crate::delta::Value::Annotation(server),
            crate::delta::Value::Annotation(local),
            "rating",
            "local",
        ) => server.rating = local.rating,
        (
            crate::delta::Value::Annotation(server),
            crate::delta::Value::Annotation(local),
            "label",
            "local",
        ) => server.label = local.label,
        (
            crate::delta::Value::Annotation(server),
            crate::delta::Value::Annotation(local),
            "keywords",
            "local",
        ) => server.keywords = local.keywords.clone(),
        (
            crate::delta::Value::Annotation(server),
            crate::delta::Value::Annotation(local),
            "keywords",
            "combined",
        ) => {
            server.keywords.extend(local.keywords.clone());
            server.keywords.sort();
            server.keywords.dedup();
        }
        (
            crate::delta::Value::Collection(server),
            crate::delta::Value::Collection(local),
            "name",
            "local",
        ) => server.name = local.name.clone(),
        (
            crate::delta::Value::Collection(server),
            crate::delta::Value::Collection(local),
            "members",
            "local",
        ) if server.rules.is_none() && local.rules.is_none() => {
            server.members = local.members.clone()
        }
        (
            crate::delta::Value::Collection(server),
            crate::delta::Value::Collection(local),
            "members",
            "combined",
        ) if server.rules.is_none() && local.rules.is_none() => {
            server.members.extend(local.members.clone());
            server.members.sort();
            server.members.dedup();
        }
        (
            crate::delta::Value::Collection(server),
            crate::delta::Value::Collection(local),
            "type",
            "local",
        ) if server.rules.is_some() != local.rules.is_some() => {
            return Err(EngineError::Unsupported(
                "manual-to-Smart Collection conversion conflicts cannot be resolved on a replica"
                    .into(),
            ));
        }
        (
            crate::delta::Value::Collection(server),
            crate::delta::Value::Collection(local),
            field,
            "local",
        ) => {
            let (Some(server_rules), Some(local_rules)) = (&mut server.rules, &local.rules) else {
                return Err(EngineError::Unsupported(
                    "Smart Collection rules cannot be merged across a type conversion".into(),
                ));
            };
            match field {
                "format" => server_rules.format = local_rules.format.clone(),
                "captured_from" => server_rules.captured_from = local_rules.captured_from.clone(),
                "captured_to" => server_rules.captured_to = local_rules.captured_to.clone(),
                "camera" => server_rules.camera = local_rules.camera.clone(),
                "lens" => server_rules.lens = local_rules.lens.clone(),
                "min_rating" => server_rules.min_rating = local_rules.min_rating,
                "max_rating" => server_rules.max_rating = local_rules.max_rating,
                "label" => server_rules.label = local_rules.label,
                "keyword" => server_rules.keyword = local_rules.keyword.clone(),
                "text" => server_rules.text = local_rules.text.clone(),
                _ => {
                    return Err(EngineError::BadParam(
                        "unsupported Collection conflict field".into(),
                    ));
                }
            }
        }
        (_, _, _, "server") => {}
        _ => {
            return Err(EngineError::BadParam(
                "choice is incompatible with the conflict field".into(),
            ));
        }
    }
    Ok(())
}

fn pending_entity(change: &crate::delta::Change) -> SyncEntity {
    match &change.value {
        crate::delta::Value::Annotation(_) => SyncEntity {
            key: change.key(),
            kind: SyncEntityKind::Asset,
            fields: vec!["rating".into(), "label".into(), "keywords".into()],
        },
        crate::delta::Value::Collection(c) => SyncEntity {
            key: change.key(),
            kind: SyncEntityKind::Collection,
            fields: if c.rules.is_some() {
                vec!["name".into(), "rules".into()]
            } else {
                vec!["name".into(), "members".into()]
            },
        },
    }
}

pub(crate) fn journal_conflicts(ctx: &Context) -> EngineResult<Vec<SyncConflict>> {
    let stored = crate::journal::conflicts(ctx)?;
    let mut out = Vec::new();
    for item in stored {
        let mut fields = Vec::new();
        let mut shared = Vec::new();
        match (&item.local.value, &item.server.value) {
            (crate::delta::Value::Annotation(local), crate::delta::Value::Annotation(server)) => {
                for (field, a, b) in [
                    (
                        "rating",
                        SyncValue::Integer(local.rating),
                        SyncValue::Integer(server.rating),
                    ),
                    (
                        "label",
                        SyncValue::Integer(local.label),
                        SyncValue::Integer(server.label),
                    ),
                    (
                        "keywords",
                        SyncValue::Set(local.keywords.clone()),
                        SyncValue::Set(server.keywords.clone()),
                    ),
                ] {
                    if a != b {
                        fields.push(SyncFieldConflict {
                            field: field.into(),
                            local: a,
                            server: b,
                            choice: choice_for(&item.choices, field),
                        });
                    } else {
                        shared.push(SyncSharedField {
                            field: field.into(),
                            value: a,
                        });
                    }
                }
            }
            (crate::delta::Value::Collection(local), crate::delta::Value::Collection(server)) => {
                if local.name != server.name {
                    fields.push(SyncFieldConflict {
                        field: "name".into(),
                        local: SyncValue::Text(local.name.clone()),
                        server: SyncValue::Text(server.name.clone()),
                        choice: choice_for(&item.choices, "name"),
                    });
                } else {
                    shared.push(SyncSharedField {
                        field: "name".into(),
                        value: SyncValue::Text(local.name.clone()),
                    });
                }
                if local.rules.is_some() != server.rules.is_some() {
                    fields.push(SyncFieldConflict {
                        field: "type".into(),
                        local: SyncValue::Text(
                            if local.rules.is_some() {
                                "smart"
                            } else {
                                "manual"
                            }
                            .into(),
                        ),
                        server: SyncValue::Text(
                            if server.rules.is_some() {
                                "smart"
                            } else {
                                "manual"
                            }
                            .into(),
                        ),
                        choice: choice_for(&item.choices, "type"),
                    });
                } else {
                    shared.push(SyncSharedField {
                        field: "type".into(),
                        value: SyncValue::Text(
                            if local.rules.is_some() {
                                "smart"
                            } else {
                                "manual"
                            }
                            .into(),
                        ),
                    });
                }
                if let (Some(local_rules), Some(server_rules)) = (&local.rules, &server.rules) {
                    let fields_to_compare = [
                        (
                            "format",
                            local_rules
                                .format
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .format
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "captured_from",
                            local_rules
                                .captured_from
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .captured_from
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "captured_to",
                            local_rules
                                .captured_to
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .captured_to
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "camera",
                            local_rules
                                .camera
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .camera
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "lens",
                            local_rules
                                .lens
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .lens
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "min_rating",
                            local_rules
                                .min_rating
                                .map(SyncValue::Integer)
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .min_rating
                                .map(SyncValue::Integer)
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "max_rating",
                            local_rules
                                .max_rating
                                .map(SyncValue::Integer)
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .max_rating
                                .map(SyncValue::Integer)
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "label",
                            local_rules
                                .label
                                .map(SyncValue::Integer)
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .label
                                .map(SyncValue::Integer)
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "keyword",
                            local_rules
                                .keyword
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .keyword
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                        (
                            "text",
                            local_rules
                                .text
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                            server_rules
                                .text
                                .as_ref()
                                .map(|v| SyncValue::Text(v.clone()))
                                .unwrap_or(SyncValue::None),
                        ),
                    ];
                    for (field, local_value, server_value) in fields_to_compare {
                        if local_value != server_value {
                            fields.push(SyncFieldConflict {
                                field: field.into(),
                                local: local_value,
                                server: server_value,
                                choice: choice_for(&item.choices, field),
                            });
                        } else {
                            shared.push(SyncSharedField {
                                field: field.into(),
                                value: local_value,
                            });
                        }
                    }
                }
                if local.members != server.members {
                    fields.push(SyncFieldConflict {
                        field: "members".into(),
                        local: SyncValue::Set(local.members.clone()),
                        server: SyncValue::Set(server.members.clone()),
                        choice: choice_for(&item.choices, "members"),
                    });
                } else {
                    shared.push(SyncSharedField {
                        field: "members".into(),
                        value: SyncValue::Set(local.members.clone()),
                    });
                }
            }
            _ => {}
        }
        let kind = match item.local.value {
            crate::delta::Value::Annotation(_) => SyncEntityKind::Asset,
            _ => SyncEntityKind::Collection,
        };
        let display_name = match &item.local.value {
            crate::delta::Value::Annotation(annotation) => ctx
                .catalog()
                .asset_by_uid(&annotation.uid)?
                .map(|asset| bounded_display_name(&asset.name))
                .unwrap_or_else(|| bounded_display_name(&annotation.uid)),
            crate::delta::Value::Collection(collection) => bounded_display_name(&collection.name),
        };
        out.push(SyncConflict {
            key: item.key,
            kind,
            display_name,
            fields,
            shared,
        });
    }
    Ok(out)
}

fn bounded_display_name(value: &str) -> String {
    const MAX_CHARS: usize = 256;
    value.chars().take(MAX_CHARS).collect()
}

fn choice_for(choices: &[(String, String)], field: &str) -> Option<SyncChoice> {
    choices
        .iter()
        .find(|(name, _)| name == field)
        .and_then(|(_, value)| match value.as_str() {
            "local" => Some(SyncChoice::Local),
            "server" => Some(SyncChoice::Server),
            "combined" => Some(SyncChoice::Combined),
            _ => None,
        })
}

impl Command for Sync {
    fn id(&self) -> &'static str {
        "sync"
    }
    fn label(&self) -> &'static str {
        "Sync this catalogue with the NAS"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "url",
                required: true,
                positional: false,
                doc: "base URL of the sync service, e.g. https://nas:8443",
            },
            crate::security::TOKEN_FILE,
            crate::security::CA_FILE,
            ParamSpec {
                name: "require-replica",
                required: false,
                positional: false,
                doc: "require an established replica before the request",
            },
            ParamSpec {
                name: "preview",
                required: false,
                positional: false,
                doc: "fetch one referenced preview by SHA-256 instead of syncing the catalogue",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> EngineResult<Outcome> {
        let base = params.require("url")?.trim().to_owned();
        validate_base(&base)?;
        let transport = UreqTransport::from_params(params, &base)?;
        if let Some(hash) = params.get("preview") {
            let fetched = client::fetch_preview(ctx, &transport, &base, hash)?;
            return Ok(Outcome::text(if fetched {
                "preview downloaded and verified"
            } else {
                "preview already cached"
            }));
        }
        client::recover_staged_snapshot(ctx)?;
        if crate::journal::has_unresolved(ctx)?
            && !matches!(
                crate::journal::recovery_phase(ctx)?.as_deref(),
                Some("applied" | "retry")
            )
        {
            return Err(EngineError::Unsupported(
                "unresolved sync conflicts are stored; use sync-resolve to choose fields and apply"
                    .into(),
            ));
        }
        let require_replica = match params.get("require-replica") {
            None | Some("false") => false,
            Some("true") => true,
            Some(_) => {
                return Err(EngineError::BadParam(
                    "require-replica must be true or false".into(),
                ));
            }
        };
        match client::sync_once_with_requirement(ctx, &transport, &base, require_replica) {
            Ok(report) => Ok(Outcome::Sync(SyncOutcome::Run(
                icatalog_engine::SyncRunResult {
                    text: render(&report).render(),
                    conflicts: journal_conflicts(ctx)?,
                    backup: report.backup.map(|path| path.display().to_string()),
                },
            ))),
            // Offline is deferred and reported, never fatal (docs/sync.md section 1).
            Err(SyncError::Unreachable(m)) => Ok(Outcome::Sync(SyncOutcome::Run(
                icatalog_engine::SyncRunResult {
                    text: format!("sync deferred: {m}"),
                    conflicts: journal_conflicts(ctx)?,
                    backup: None,
                },
            ))),
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
            crate::security::TOKEN_FILE,
            ParamSpec {
                name: "tls-cert",
                required: false,
                positional: false,
                doc: "PEM server certificate chain; requires tls-key",
            },
            ParamSpec {
                name: "tls-key",
                required: false,
                positional: false,
                doc: "unencrypted PEM server private key; requires tls-cert",
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
                )));
            }
            None => {
                return Err(EngineError::BadParam(
                    "missing required parameter 'port'".into(),
                ));
            }
        };
        let bind = params.get("bind").unwrap_or("127.0.0.1").trim();
        if bind.is_empty() {
            return Err(EngineError::BadParam("bind must not be empty".into()));
        }
        let mut security = crate::security::ServerSecurity::from_params(params, bind)?;
        let scheme = if security.uses_tls() { "https" } else { "http" };
        let addr = if bind.contains(':') && !bind.starts_with('[') {
            format!("[{bind}]:{port}")
        } else {
            format!("{bind}:{port}")
        };
        let server = server::bind_with_security(&addr, &mut security)?;
        println!(
            "{} sync service listening on {scheme}://{addr}",
            icatalog_catalog::APP_NAME
        );
        server::serve_loop_with_security(server, ctx, security)?;
        Ok(Outcome::text("sync service stopped"))
    }
}

pub fn validate_base(base: &str) -> EngineResult<()> {
    crate::security::parse_url(base, false)
        .map(|_| ())
        .map_err(Into::into)
}

fn render(r: &SyncReport) -> Outcome {
    let mut lines = Vec::new();
    if r.initial {
        lines.push("initial sync: established the server state; earlier local edits are preserved in the backup and were not uploaded".to_owned());
    }
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
                "conflicting user edits preserved in backup; local copy resynced at generation {}",
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
    if matches!(r.action, SyncAction::Pushed) {
        if let Some(b) = &r.backup {
            lines.push(format!("backup: {} (local copy before sync)", b.display()));
        }
    }
    for key in &r.conflicts {
        lines.push(format!("conflict: {key}"));
    }
    if r.sent_bytes > 0 {
        lines.push(format!("user-edit upload: {} bytes", r.sent_bytes));
    }
    if r.bytes > 0 {
        lines.push(format!("snapshot: {} bytes", r.bytes));
    }
    Outcome::text(lines.join("\n"))
}
