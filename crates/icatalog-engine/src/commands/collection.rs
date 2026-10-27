use super::{asset_table, is_initialised, require_catalogue};
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use icatalog_catalog::AssetQuery;

pub(crate) struct Collection;

impl Command for Collection {
    fn id(&self) -> &'static str {
        "collection"
    }

    fn label(&self) -> &'static str {
        "Create, browse and edit manual collections"
    }

    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "action",
                required: true,
                positional: true,
                doc: "list | create | show | add | remove (after the catalogue path)",
            },
            ParamSpec {
                name: "name",
                required: false,
                positional: false,
                doc: "collection name for create (1-1024 bytes after trimming)",
            },
            ParamSpec {
                name: "id",
                required: false,
                positional: false,
                doc: "collection id for show/add/remove (see collection list)",
            },
            ParamSpec {
                name: "asset",
                required: false,
                positional: false,
                doc: "asset id for add/remove (see list); repeated operations are harmless",
            },
            ParamSpec {
                name: "assets",
                required: false,
                positional: false,
                doc: "comma-separated asset ids for atomic add/remove; use asset or assets, not both (up to 32000 ids)",
            },
        ]
    }

    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }

    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        require_catalogue(ctx)?;
        let action = params.require("action")?;
        let allowed: &[&str] = match action {
            "list" => &[],
            "create" => &["name"],
            "show" => &["id"],
            "add" | "remove" => &["id", "asset", "assets"],
            _ => {
                return Err(EngineError::BadParam(
                    "collection action must be list, create, show, add or remove".into(),
                ))
            }
        };
        for key in ["name", "id", "asset", "assets"] {
            if params.get(key).is_some() && !allowed.contains(&key) {
                return Err(EngineError::BadParam(format!(
                    "--{key} is not used by collection {action}"
                )));
            }
        }
        match action {
            "list" => Ok(collection_table(ctx.catalog().all_collections()?)),
            "create" => {
                let name = params.require("name")?.trim();
                if name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
                    return Err(EngineError::BadParam(
                        "collection name must be 1-1024 bytes and contain no control characters"
                            .into(),
                    ));
                }
                let id = ctx.catalog().insert_collection(name)?;
                Ok(collection_table(vec![icatalog_catalog::Collection {
                    id,
                    name: name.into(),
                }]))
            }
            "show" => {
                let id = require_id(ctx, params, "id")?;
                Ok(asset_table(ctx.catalog().query_assets(&AssetQuery {
                    collection_id: Some(id),
                    ..Default::default()
                })?))
            }
            "add" | "remove" => {
                let id = require_id(ctx, params, "id")?;
                let assets = asset_ids(params)?;
                for asset in &assets {
                    if ctx.catalog().asset(*asset)?.is_none() {
                        return Err(EngineError::BadParam(format!("no such asset: {asset}")));
                    }
                }
                for asset in &assets {
                    if action == "add" {
                        ctx.catalog().add_to_collection(id, *asset)?;
                    } else {
                        ctx.catalog().remove_from_collection(id, *asset)?;
                    }
                }
                let state = if action == "add" { "in" } else { "not in" };
                if assets.len() == 1 {
                    if let Some(asset) = assets.first() {
                        return Ok(Outcome::text(format!(
                            "asset {asset} is {state} collection {id}"
                        )));
                    }
                }
                Ok(Outcome::text(format!(
                    "{} assets are {state} collection {id}",
                    assets.len()
                )))
            }
            _ => Err(EngineError::BadParam("invalid collection action".into())),
        }
    }
}

fn asset_ids(params: &Params) -> Result<Vec<i64>> {
    let Some(raw) = params.get("assets") else {
        return Ok(vec![positive_id(params, "asset")?]);
    };
    if params.get("asset").is_some() {
        return Err(EngineError::BadParam(
            "use either asset or assets, not both".into(),
        ));
    }
    if raw.len() > 704_000 {
        return Err(EngineError::BadParam(
            "assets is too long (maximum 32000 ids)".into(),
        ));
    }
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (index, value) in raw.split(',').enumerate() {
        if index >= 32_000 {
            return Err(EngineError::BadParam(
                "select at most 32000 assets at once".into(),
            ));
        }
        let id = value
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or_else(|| {
                EngineError::BadParam(
                    "assets must be a comma-separated list of positive integer ids".into(),
                )
            })?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn collection_table(collections: Vec<icatalog_catalog::Collection>) -> Outcome {
    Outcome::table(
        vec!["id".into(), "name".into()],
        collections
            .into_iter()
            .map(|c| vec![c.id.to_string(), c.name])
            .collect(),
    )
}

fn positive_id(params: &Params, key: &str) -> Result<i64> {
    let id = params
        .get_i64(key)?
        .ok_or_else(|| EngineError::BadParam(format!("missing required parameter '{key}'")))?;
    if id <= 0 {
        return Err(EngineError::BadParam(format!(
            "{key} must be a positive integer"
        )));
    }
    Ok(id)
}

fn require_id(ctx: &Context, params: &Params, key: &str) -> Result<i64> {
    let id = positive_id(params, key)?;
    if ctx.catalog().collection(id)?.is_none() {
        return Err(EngineError::BadParam(format!("no such collection: {id}")));
    }
    Ok(id)
}

pub(super) fn filter_id(ctx: &Context, params: &Params) -> Result<Option<i64>> {
    params
        .get("collection")
        .map(|_| require_id(ctx, params, "collection"))
        .transpose()
}
