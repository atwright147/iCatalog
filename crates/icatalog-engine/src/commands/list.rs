use super::{asset_table, is_initialised};
use crate::{Command, Context, Outcome, ParamSpec, Params, Result};
use icatalog_catalog::AssetQuery;

pub(crate) struct List;

impl Command for List {
    fn id(&self) -> &'static str {
        "list"
    }
    fn label(&self) -> &'static str {
        "List assets"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "limit",
            required: false,
            doc: "maximum number of assets to list",
        }]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        let limit = params.get_i64("limit")?;
        let q = AssetQuery {
            limit,
            ..Default::default()
        };
        Ok(asset_table(ctx.catalog().query_assets(&q)?))
    }
}
