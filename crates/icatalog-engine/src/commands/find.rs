use super::{asset_table, is_initialised};
use crate::{Command, Context, Outcome, ParamSpec, Params, Result};
use icatalog_catalog::AssetQuery;

pub(crate) struct Find;

impl Command for Find {
    fn id(&self) -> &'static str {
        "find"
    }
    fn label(&self) -> &'static str {
        "Find assets"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "query",
                required: true,
                positional: true,
                doc: "substring to match against asset name or path",
            },
            ParamSpec {
                name: "keyword",
                required: false,
                positional: false,
                doc: "keyword the asset must carry",
            },
            ParamSpec {
                name: "rating",
                required: false,
                positional: false,
                doc: "minimum rating 0-5",
            },
            ParamSpec {
                name: "label",
                required: false,
                positional: false,
                doc: "exact label 0-7",
            },
            ParamSpec {
                name: "limit",
                required: false,
                positional: false,
                doc: "maximum number of assets to list",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        let query = params.require("query")?.to_owned();
        let keyword = params.get("keyword").map(str::to_owned);
        let min_rating = params.get_i64("rating")?;
        let label = params.get_i64("label")?;
        let limit = params.get_i64("limit")?;
        let q = AssetQuery {
            text_like: Some(query),
            keyword,
            min_rating,
            label,
            limit,
            ..Default::default()
        };
        Ok(asset_table(ctx.catalog().query_assets(&q)?))
    }
}
