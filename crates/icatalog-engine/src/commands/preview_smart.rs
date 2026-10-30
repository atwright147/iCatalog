//! Read-only preview of unsaved Smart Collection rules.
use crate::{Context, Outcome, Params, Result};

pub(super) fn run(ctx: &Context, params: &Params) -> Result<Outcome> {
    let rules = super::smart_collection::parse(params)?;
    let count = ctx.catalog().count_smart_matches(&rules)?;
    let mut clauses = Vec::new();
    for (prefix, value) in [
        ("Format is", &rules.format),
        ("Captured on or after", &rules.captured_from),
        ("Captured on or before", &rules.captured_to),
        ("Camera contains", &rules.camera),
        ("Lens contains", &rules.lens),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{prefix} {value:?}"));
        }
    }
    if let Some(rating) = rules.min_rating {
        clauses.push(format!("Rating is at least {rating} out of 5"));
    }
    if let Some(label) = rules.label {
        clauses.push(format!("Label is {label}"));
    }
    if let Some(keyword) = &rules.keyword {
        clauses.push(format!("Keyword is {keyword:?}"));
    }
    if let Some(text) = &rules.text {
        clauses.push(format!("Filename or path contains {text:?}"));
    }
    let noun = if count == 1 { "item" } else { "items" };
    Ok(Outcome::text(format!(
        "{count} matching {noun}\n{}",
        clauses.join(" AND ")
    )))
}
