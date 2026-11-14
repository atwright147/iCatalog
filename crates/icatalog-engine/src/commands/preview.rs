use super::is_initialised;
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, PreviewImage, Result};

pub(crate) struct Preview;

impl Command for Preview {
    fn id(&self) -> &'static str {
        "preview"
    }
    fn label(&self) -> &'static str {
        "View a stored preview"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "id",
            required: true,
            positional: false,
            doc: "asset ID; reads verified stored pixels only (CLI prints preview details)",
        }]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        let id = params
            .get_i64("id")?
            .filter(|id| *id > 0)
            .ok_or_else(|| EngineError::BadParam("id must be a positive asset ID".into()))?;
        let asset = ctx
            .catalog()
            .asset(id)?
            .ok_or_else(|| EngineError::BadParam("asset does not exist".into()))?;
        let reference = ctx.catalog().preview(id)?.ok_or_else(|| {
            EngineError::Unsupported(
                "No stored preview. Run `previews <catalogue> generate` on the server or standalone catalogue with the originals available; then sync replicas.".into(),
            )
        })?;
        if !(1..=1920).contains(&reference.width) || !(1..=1920).contains(&reference.height) {
            return Err(EngineError::Unsupported("Stored preview dimensions are invalid; regenerate the preview on the server or standalone catalogue.".into()));
        }
        let bytes = crate::previews::read(ctx.catalog(), ctx.path(), &reference.hash)?
            .ok_or_else(|| EngineError::Unsupported(format!(
                "Preview is not cached. Fetch it with `sync <catalogue> --url <server> --preview {}` (include your security file options), or generate previews where the originals are available.", reference.hash
            )))?;
        // Preview generation already applied orientation. Never rotate it a second time.
        let decoded = icatalog_media::decode_preview_rgba(&bytes)
            .map_err(|error| EngineError::Unsupported(format!("Cannot decode stored preview: {error}. Regenerate previews on the server or standalone catalogue.")))?;
        if i64::from(decoded.width) != reference.width
            || i64::from(decoded.height) != reference.height
        {
            return Err(EngineError::Unsupported("Stored preview dimensions do not match its pixels; regenerate the preview on the server or standalone catalogue.".into()));
        }
        Ok(Outcome::Preview(PreviewImage {
            asset_id: id,
            filename: asset.name,
            hash: reference.hash,
            width: decoded.width,
            height: decoded.height,
            rgba: decoded.rgba,
        }))
    }
}
