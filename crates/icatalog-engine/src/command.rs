use crate::context::Context;
use crate::error::Result;
use crate::params::Params;

/// Documentation for one command parameter.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub name: &'static str,
    pub required: bool,
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
