mod find;
mod init;
mod list;
mod open;
mod status;

use crate::{Command, Context, EngineError, Result};

pub(crate) static ALL: &[&dyn Command] = &[
    &init::Init,
    &open::Open,
    &list::List,
    &find::Find,
    &status::Status,
];

/// Return the catalogue row, or an error if the file is not initialised.
fn require_catalogue(ctx: &Context) -> Result<icatalog_catalog::CatalogueRow> {
    ctx.catalog().catalogue()?.ok_or_else(|| {
        EngineError::Unsupported("not an iCatalog catalogue (run `init` first)".into())
    })
}

fn is_initialised(ctx: &Context) -> bool {
    matches!(ctx.catalog().catalogue(), Ok(Some(_)))
}

fn asset_table(assets: Vec<icatalog_catalog::Asset>) -> crate::Outcome {
    let rows = assets
        .into_iter()
        .map(|a| {
            vec![
                a.id.to_string(),
                a.name,
                a.path,
                a.format.unwrap_or_default(),
            ]
        })
        .collect();
    crate::Outcome::table(
        vec!["id".into(), "name".into(), "path".into(), "format".into()],
        rows,
    )
}
