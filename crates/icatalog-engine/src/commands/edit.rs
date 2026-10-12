use super::{is_initialised, require_catalogue};
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use icatalog_catalog::Annotation;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct Edit;

impl Command for Edit {
    fn id(&self) -> &'static str {
        "edit"
    }
    fn label(&self) -> &'static str {
        "Edit an asset's keywords, rating and label"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "id",
                required: true,
                positional: false,
                doc: "asset row id to edit (see `list`)",
            },
            ParamSpec {
                name: "rating",
                required: false,
                positional: false,
                doc: "set rating 0-5",
            },
            ParamSpec {
                name: "label",
                required: false,
                positional: false,
                doc: "set label 0-7",
            },
            ParamSpec {
                name: "keyword",
                required: false,
                positional: false,
                doc: "add keyword(s); comma-separated for several",
            },
            ParamSpec {
                name: "remove-keyword",
                required: false,
                positional: false,
                doc: "remove keyword(s); comma-separated for several",
            },
            ParamSpec {
                name: "clear-keywords",
                required: false,
                positional: false,
                doc: "remove every keyword ('true')",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        require_catalogue(ctx)?;
        let id = params
            .get_i64("id")?
            .ok_or_else(|| EngineError::BadParam("missing required parameter 'id'".into()))?;
        let asset = ctx
            .catalog()
            .asset(id)?
            .ok_or_else(|| EngineError::BadParam(format!("no such asset: {id}")))?;

        let rating = params.get("rating").map(str::to_owned);
        let label = params.get("label").map(str::to_owned);
        let add_names = names(params.get("keyword"), "--keyword")?;
        let remove_names = names(params.get("remove-keyword"), "--remove-keyword")?;
        let clear = match params.get("clear-keywords") {
            None => false,
            Some("true") => true,
            Some("false") => false,
            Some(v) => {
                return Err(EngineError::BadParam(format!(
                    "--clear-keywords must be 'true' or 'false' (got '{v}')"
                )));
            }
        };
        if rating.is_none()
            && label.is_none()
            && add_names.is_empty()
            && remove_names.is_empty()
            && !clear
        {
            return Err(EngineError::BadParam(
                "no edits requested (--rating, --label, --keyword, --remove-keyword or --clear-keywords)"
                    .into(),
            ));
        }

        let mut changes: Vec<String> = Vec::new();
        let existing = ctx.catalog().asset_keywords(id)?;
        let mut present: HashSet<String> = existing.iter().map(|k| k.name.clone()).collect();

        for name in add_names {
            if present.contains(&name) {
                continue;
            }
            let kid = ctx.catalog().ensure_keyword(&name, None)?;
            ctx.catalog().add_asset_keyword(id, kid)?;
            present.insert(name.clone());
            changes.push(format!("+{name}"));
        }
        for name in remove_names {
            let Some(k) = existing.iter().find(|k| k.name == name) else {
                continue;
            };
            ctx.catalog().remove_asset_keyword(id, k.id)?;
            changes.push(format!("-{name}"));
        }
        if clear && !existing.is_empty() {
            for k in &existing {
                ctx.catalog().remove_asset_keyword(id, k.id)?;
            }
            changes.push("keywords cleared".to_string());
        }

        if rating.is_some() || label.is_some() {
            let mut ann = ctx.catalog().annotation(id)?.unwrap_or(Annotation {
                asset_id: id,
                rating: 0,
                label: 0,
                caption: None,
                headline: None,
                creator: None,
                copyright: None,
                updated_utc: None,
            });
            if let Some(v) = &rating {
                let r = bounded(v, "rating", 5)?;
                if r != ann.rating {
                    ann.rating = r;
                    changes.push(format!("rating {r}"));
                }
            }
            if let Some(v) = &label {
                let l = bounded(v, "label", 7)?;
                if l != ann.label {
                    ann.label = l;
                    changes.push(format!("label {l}"));
                }
            }
            ann.updated_utc = Some(utc_iso_now());
            ctx.catalog().set_annotation(&ann)?;
        }

        if changes.is_empty() {
            return Ok(Outcome::text(format!(
                "asset {id} ({}) already has these values; nothing changed",
                asset.name
            )));
        }
        Ok(Outcome::text(format!(
            "edited asset {id} ({}): {}",
            asset.name,
            changes.join(", ")
        )))
    }
}

/// Split a comma-separated keyword parameter into trimmed, non-empty names.
fn names(v: Option<&str>, option: &str) -> Result<Vec<String>> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let out: Vec<String> = v
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if out.is_empty() {
        return Err(EngineError::BadParam(format!(
            "{option} needs at least one name"
        )));
    }
    Ok(out)
}

/// Parse `0..=max`; anything else (non-integer or out of range) is a `BadParam`.
fn bounded(v: &str, name: &str, max: i64) -> Result<i64> {
    let n = v.parse::<i64>().map_err(|_| {
        EngineError::BadParam(format!("{name} must be an integer 0-{max} (got '{v}')"))
    })?;
    if !(0..=max).contains(&n) {
        return Err(EngineError::BadParam(format!(
            "{name} must be an integer 0-{max} (got '{v}')"
        )));
    }
    Ok(n)
}

/// Current time as `YYYY-MM-DDTHH:MM:SSZ` (UTC).
fn utc_iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Day number to a civil (proleptic Gregorian) date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
