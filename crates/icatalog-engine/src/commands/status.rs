use super::{is_initialised, require_catalogue};
use crate::{Command, Context, Outcome, ParamSpec, Params, Result};

pub(crate) struct Status;

impl Command for Status {
    fn id(&self) -> &'static str {
        "status"
    }
    fn label(&self) -> &'static str {
        "Show catalogue statistics"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, _params: &Params) -> Result<Outcome> {
        require_catalogue(ctx)?;
        let s = ctx.catalog().stats()?;
        let row = |k: &str, v: String| vec![k.to_string(), v];
        let rows = vec![
            row("assets", s.assets.to_string()),
            row("folders", s.folders.to_string()),
            row("collections", s.collections.to_string()),
            row("keywords", s.keywords.to_string()),
            row("thumbnails", s.thumbnails.to_string()),
            row(
                "generation",
                s.generation
                    .map(|g| g.to_string())
                    .unwrap_or_else(|| "-".into()),
            ),
            row("schema_version", s.schema_version.to_string()),
        ];
        Ok(Outcome::table(vec!["metric".into(), "value".into()], rows))
    }
}
