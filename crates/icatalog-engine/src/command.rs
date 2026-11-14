use crate::context::Context;
use crate::error::Result;
use crate::params::Params;

/// Documentation for one command parameter.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub name: &'static str,
    pub required: bool,
    /// Passed as a positional argument rather than `--name <value>`.
    pub positional: bool,
    pub doc: &'static str,
}

/// A user-visible action.
pub trait Command: Sync {
    /// Stable machine id.
    fn id(&self) -> &'static str;
    /// Human label.
    fn label(&self) -> &'static str;
    /// Parameter documentation.
    fn params(&self) -> &'static [ParamSpec];
    /// Whether the command can run on this context.
    fn enabled(&self, ctx: &Context) -> bool {
        let _ = ctx;
        true
    }
    /// Execute the command.
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome>;
}

/// Result of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Text(String),
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// Structured sync state for native controls. CLI rendering remains stable.
    Sync(SyncOutcome),
    /// Verified stored preview pixels for a native viewer; CLI renders metadata only.
    Preview(PreviewImage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    pub asset_id: i64,
    pub filename: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Structured result returned by the L3 sync commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Check(SyncCheckResult),
    Run(SyncRunResult),
    Resolve(SyncResolveResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncCheckStatus {
    Reachable,
    Untracked,
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCheckResult {
    pub status: SyncCheckStatus,
    pub catalogue_id: String,
    pub baseline_generation: Option<i64>,
    pub local_generation: Option<i64>,
    pub server_generation: Option<i64>,
    pub replica: bool,
    pub sync_required: bool,
    pub pending: Vec<SyncEntity>,
    pub conflicts: Vec<SyncConflict>,
    pub recovery_backup: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRunResult {
    pub text: String,
    pub conflicts: Vec<SyncConflict>,
    pub backup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncResolveResult {
    pub conflicts: Vec<SyncConflict>,
    pub applied: bool,
    pub recovery_backup: Option<std::path::PathBuf>,
    pub session_id: Option<String>,
    pub total_conflicts: usize,
    pub page: usize,
    pub page_size: usize,
    pub all_choices_made: bool,
    pub recovery_phase: Option<String>,
    pub recovery_pending: bool,
    pub recovered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEntity {
    pub key: String,
    pub kind: SyncEntityKind,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncEntityKind {
    Asset,
    Collection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConflict {
    pub key: String,
    pub kind: SyncEntityKind,
    pub display_name: String,
    pub fields: Vec<SyncFieldConflict>,
    pub shared: Vec<SyncSharedField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSharedField {
    pub field: String,
    pub value: SyncValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncFieldConflict {
    pub field: String,
    pub local: SyncValue,
    pub server: SyncValue,
    pub choice: Option<SyncChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncValue {
    Text(String),
    Integer(i64),
    Set(Vec<String>),
    Rules(SyncRules),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRules {
    pub format: Option<String>,
    pub captured_from: Option<String>,
    pub captured_to: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub min_rating: Option<i64>,
    pub max_rating: Option<i64>,
    pub label: Option<i64>,
    pub keyword: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncChoice {
    Local,
    Server,
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    Preparing,
    Checking,
    Uploading,
    Downloading,
    Validating,
    BackingUp,
    Replacing,
    Complete,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncProgress {
    pub phase: SyncPhase,
    pub completed: Option<u64>,
    pub total: Option<u64>,
}

impl Outcome {
    /// Text outcome.
    pub fn text(s: impl Into<String>) -> Self {
        Outcome::Text(s.into())
    }

    /// Table outcome.
    pub fn table(columns: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Outcome::Table { columns, rows }
    }

    /// Plain-text rendering; tables are aligned columns.
    pub fn render(&self) -> String {
        match self {
            Outcome::Text(s) => s.clone(),
            Outcome::Preview(image) => format!(
                "preview: {} ({} × {} pixels); asset {}; SHA-256 {}",
                image.filename, image.width, image.height, image.asset_id, image.hash
            ),
            Outcome::Sync(SyncOutcome::Run(result)) => result.text.clone(),
            Outcome::Sync(SyncOutcome::Check(result)) => match &result.status {
                SyncCheckStatus::Reachable => format!(
                    "server reachable; catalogue {}; baseline generation {}; local generation {}; server generation {}; {} pending edits{}",
                    result.catalogue_id,
                    result
                        .baseline_generation
                        .map_or_else(|| "none".into(), |n| n.to_string()),
                    result
                        .local_generation
                        .map_or_else(|| "unknown".into(), |n| n.to_string()),
                    result
                        .server_generation
                        .map_or_else(|| "unknown".into(), |n| n.to_string()),
                    result.pending.len(),
                    if result.sync_required {
                        "; sync required"
                    } else {
                        ""
                    }
                ),
                SyncCheckStatus::Untracked => format!(
                    "catalogue {} is not a replica; first sync is required before desktop checks can report server reachability",
                    result.catalogue_id
                ),
                SyncCheckStatus::Unreachable(reason) => format!(
                    "server unreachable for catalogue {}; baseline generation {}; local generation {}: {reason}",
                    result.catalogue_id,
                    result
                        .baseline_generation
                        .map_or_else(|| "none".into(), |n| n.to_string()),
                    result
                        .local_generation
                        .map_or_else(|| "unknown".into(), |n| n.to_string())
                ),
            },
            Outcome::Sync(SyncOutcome::Resolve(result)) => render_resolve(result),
            Outcome::Table { columns, rows } => {
                let mut widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
                for row in rows {
                    for (i, cell) in row.iter().enumerate() {
                        let n = cell.chars().count();
                        match widths.get_mut(i) {
                            Some(w) => *w = (*w).max(n),
                            None => widths.push(n),
                        }
                    }
                }
                let line = |cells: &[String]| -> String {
                    let mut out = String::new();
                    for (i, w) in widths.iter().enumerate() {
                        if i > 0 {
                            out.push_str("  ");
                        }
                        let cell = cells.get(i).map(String::as_str).unwrap_or("");
                        out.push_str(cell);
                        for _ in cell.chars().count()..*w {
                            out.push(' ');
                        }
                    }
                    out.trim_end().to_string()
                };
                let mut lines = vec![line(columns)];
                lines.extend(rows.iter().map(|r| line(r)));
                lines.join("\n")
            }
        }
    }
}

fn render_resolve(result: &SyncResolveResult) -> String {
    let mut out = if result.applied {
        "Conflict choices were applied and synced successfully.".to_owned()
    } else {
        format!(
            "{} unresolved conflict(s); showing {} on page {} of {}.",
            result.total_conflicts,
            result.conflicts.len(),
            result.page.saturating_add(1),
            result.total_conflicts.div_ceil(result.page_size.max(1)),
        )
    };
    if let Some(session) = &result.session_id {
        out.push_str(&format!(
            "\nSession: {session} (pass --session-id {session} to show, choose or apply)."
        ));
    }
    if result.recovery_pending {
        out.push_str(&format!(
            "\nInterrupted sync recovery is pending{}; run `sync-resolve recover` before editing.",
            result
                .recovery_phase
                .as_deref()
                .map_or(String::new(), |phase| format!(" (phase: {phase})"))
        ));
    } else if result.recovered {
        out.push_str(
            "\nInterrupted sync recovery completed; reopen the catalogue view before continuing.",
        );
    }
    out.push_str(if result.all_choices_made {
        "\nAll conflicting fields have choices."
    } else {
        "\nChoose a value for every conflicting field before applying."
    });
    for conflict in &result.conflicts {
        out.push_str(&format!(
            "\n\n{} — {} [{}]",
            conflict.display_name,
            conflict.key,
            match conflict.kind {
                SyncEntityKind::Asset => "asset",
                SyncEntityKind::Collection => "collection",
            }
        ));
        for field in &conflict.fields {
            out.push_str(&format!(
                "\n  {}: local={}, server={}{}",
                field.field,
                render_sync_value(&field.local),
                render_sync_value(&field.server),
                field
                    .choice
                    .map_or_else(String::new, |choice| format!("; choice={choice:?}"))
            ));
        }
        if !conflict.shared.is_empty() {
            out.push_str("\n  Shared:");
            for field in &conflict.shared {
                out.push_str(&format!(
                    "\n    {}={}",
                    field.field,
                    render_sync_value(&field.value)
                ));
            }
        }
    }
    out
}

fn render_sync_value(value: &SyncValue) -> String {
    fn shorten(value: &str, max: usize) -> String {
        let mut chars = value.chars();
        let mut output: String = chars.by_ref().take(max).collect();
        if chars.next().is_some() {
            output.push('…');
        }
        output
    }
    match value {
        SyncValue::Text(value) => format!("\"{}\"", shorten(value, 96)),
        SyncValue::Integer(value) => value.to_string(),
        SyncValue::Set(values) => {
            let mut rendered: Vec<_> = values.iter().take(5).map(|v| shorten(v, 48)).collect();
            if values.len() > rendered.len() {
                rendered.push(format!("… +{} more", values.len() - rendered.len()));
            }
            format!("{{{}}}", rendered.join(", "))
        }
        SyncValue::Rules(rules) => format!(
            "rules(min_rating={:?}, max_rating={:?}, label={:?}, format={})",
            rules.min_rating,
            rules.max_rating,
            rules.label,
            rules
                .format
                .as_deref()
                .map_or("None".to_owned(), |v| shorten(v, 48))
        ),
        SyncValue::None => "(none)".into(),
    }
}
