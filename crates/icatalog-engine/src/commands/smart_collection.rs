use crate::{Context, EngineError, Outcome, Params, Result};
use icatalog_catalog::SmartRules;

pub(super) const RULE_KEYS: &[&str] = &[
    "format",
    "captured-from",
    "captured-to",
    "camera",
    "lens",
    "min-rating",
    "label",
    "keyword",
    "text",
];

pub(super) fn parse(params: &Params) -> Result<SmartRules> {
    let text = |key| params.get(key).map(|s| s.trim().to_owned());
    let rules = SmartRules {
        format: text("format"),
        captured_from: text("captured-from"),
        captured_to: text("captured-to"),
        camera: text("camera"),
        lens: text("lens"),
        min_rating: params.get_i64("min-rating")?,
        label: params.get_i64("label")?,
        keyword: text("keyword"),
        text: text("text"),
    };
    rules
        .validate()
        .map_err(|e| EngineError::BadParam(e.to_string()))?;
    Ok(rules)
}

pub(super) fn run(ctx: &mut Context, params: &Params) -> Result<Outcome> {
    match params.require("action")? {
        "create-smart" => {
            let name = super::collection::name(params)?;
            let rules = parse(params)?;
            let id = ctx.catalog().insert_collection(name)?;
            ctx.catalog().set_smart_rules(id, &rules)?;
            Ok(Outcome::text(format!(
                "Created Smart Collection {id}: {name}"
            )))
        }
        "rules" | "set-rules" => {
            let id = super::collection::require_id(ctx, params, "id")?;
            let Some(mut rules) = ctx.catalog().smart_rules(id)? else {
                return Err(EngineError::BadParam(
                    "this is a manual collection; create a Smart Collection to use rules".into(),
                ));
            };
            if params.require("action")? == "set-rules" {
                rules = parse(params)?;
                if params.get("name").is_some() {
                    let name = super::collection::name(params)?;
                    ctx.catalog().rename_collection(id, name)?;
                }
                ctx.catalog().set_smart_rules(id, &rules)?;
            }
            let pairs = [
                ("format", rules.format),
                ("captured-from", rules.captured_from),
                ("captured-to", rules.captured_to),
                ("camera", rules.camera),
                ("lens", rules.lens),
                ("min-rating", rules.min_rating.map(|v| v.to_string())),
                ("label", rules.label.map(|v| v.to_string())),
                ("keyword", rules.keyword),
                ("text", rules.text),
            ];
            Ok(Outcome::table(
                vec!["rule".into(), "value".into()],
                pairs
                    .into_iter()
                    .filter_map(|(k, v)| v.map(|v| vec![k.into(), v]))
                    .collect(),
            ))
        }
        _ => Err(EngineError::BadParam(
            "invalid Smart Collection action".into(),
        )),
    }
}
