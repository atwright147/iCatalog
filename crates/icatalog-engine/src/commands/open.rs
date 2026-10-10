use super::{is_initialised, require_catalogue};
use crate::{Command, Context, Outcome, ParamSpec, Params, Result};

pub(crate) struct Open;

impl Command for Open {
    fn id(&self) -> &'static str {
        "open"
    }
    fn label(&self) -> &'static str {
        "Open and describe a catalogue"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, _params: &Params) -> Result<Outcome> {
        let row = require_catalogue(ctx)?;
        let s = ctx.catalog().stats()?;
        let generation = s
            .generation
            .map(|g| g.to_string())
            .unwrap_or_else(|| "-".into());
        Ok(Outcome::text(format!(
            "path: {}\nid: {}\ngeneration: {}\nschema version: {}\nassets: {}\nfolders: {}\nkeywords: {}\ncollections: {}",
            ctx.path().display(),
            row.id,
            generation,
            s.schema_version,
            s.assets,
            s.folders,
            s.keywords,
            s.collections
        )))
    }
}
