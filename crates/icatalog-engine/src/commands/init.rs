use crate::{time, Command, Context, Outcome, ParamSpec, Params, Result};

pub(crate) struct Init;

impl Command for Init {
    fn id(&self) -> &'static str {
        "init"
    }
    fn label(&self) -> &'static str {
        "Initialise a catalogue"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "id",
                required: false,
                positional: false,
                doc: "catalogue id; generated if omitted",
            },
            ParamSpec {
                name: "created",
                required: false,
                positional: false,
                doc: "creation time (ISO-8601 UTC); now if omitted",
            },
        ]
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        if let Some(row) = ctx.catalog().catalogue()? {
            return Ok(Outcome::text(format!(
                "catalogue already initialised: {}",
                row.id
            )));
        }
        let id = params
            .get("id")
            .map(str::to_owned)
            .unwrap_or_else(time::generate_catalogue_id);
        let created = params
            .get("created")
            .map(str::to_owned)
            .unwrap_or_else(time::utc_now_iso8601);
        ctx.catalog().insert_catalogue(&id, &created)?;
        Ok(Outcome::text(format!(
            "initialised catalogue '{id}' at {created}"
        )))
    }
}
