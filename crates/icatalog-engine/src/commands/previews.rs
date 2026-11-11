use super::is_initialised;
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use std::io::Read;

pub(crate) struct Previews;
impl Command for Previews {
    fn id(&self) -> &'static str {
        "previews"
    }
    fn label(&self) -> &'static str {
        "Generate and manage catalogue previews"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "action",
            required: true,
            positional: true,
            doc: "status | generate | external | embed; generation uses stored media paths",
        }]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, p: &Params) -> Result<Outcome> {
        let action = p.require("action")?;
        if action == "status" {
            return Ok(Outcome::text(format!(
                "previews: {}; {} referenced blobs",
                ctx.catalog().preview_mode()?,
                ctx.catalog().preview_hashes()?.len()
            )));
        }
        if !matches!(action, "generate" | "external" | "embed") {
            return Err(EngineError::BadParam(
                "previews action must be status, generate, external or embed".into(),
            ));
        }
        if ctx.catalog().is_replica()? {
            return Err(EngineError::Unsupported("generate or convert previews on the server or a standalone catalogue; replicas fetch previews with sync --preview".into()));
        }
        if action != "generate" {
            let mode = if action == "embed" {
                "embedded"
            } else {
                "external"
            };
            if mode == ctx.catalog().preview_mode()? {
                ctx.catalog().prune_unreferenced_preview_blobs()?;
                return Ok(Outcome::text(format!("previews already {mode}")));
            }
            for hash in ctx.catalog().preview_hashes()? {
                let bytes =
                    crate::previews::read(ctx.catalog(), ctx.path(), &hash)?.ok_or_else(|| {
                        EngineError::Unsupported(format!(
                            "preview {hash} is missing; conversion cancelled"
                        ))
                    })?;
                crate::previews::store(ctx.catalog(), ctx.path(), &hash, &bytes, mode)?;
            }
            ctx.catalog().set_preview_mode(mode)?;
            ctx.catalog().prune_unreferenced_preview_blobs()?;
            return Ok(Outcome::text(format!("previews stored {mode}")));
        }
        let assets = ctx.catalog().query_assets(&Default::default())?;
        let mode = ctx.catalog().preview_mode()?;
        let (mut generated, mut cached, mut failed) = (0usize, 0usize, 0usize);
        for asset in assets {
            if let Some(p) = ctx.catalog().preview(asset.id)? {
                if Some(p.source_hash.as_str()) == asset.content_hash.as_deref()
                    && crate::previews::read_recoverable(ctx.catalog(), ctx.path(), &p.hash)?
                        .is_some()
                {
                    cached += 1;
                    continue;
                }
            }
            // Media paths are catalogued data, explicitly used only by Generate.
            let file = match std::fs::File::open(&asset.path) {
                Ok(f) => f,
                Err(_) => {
                    failed += 1;
                    continue;
                }
            };
            if file.metadata()?.len() > 512 * 1024 * 1024 {
                failed += 1;
                continue;
            }
            let mut bytes = Vec::new();
            file.take(512 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 512 * 1024 * 1024 {
                failed += 1;
                continue;
            }
            let source_hash = icatalog_media::content_hash(&bytes);
            if asset
                .content_hash
                .as_ref()
                .is_some_and(|h| h != &source_hash)
            {
                failed += 1;
                continue;
            }
            // TIFF headers also identify ordinary raster TIFFs; use the format
            // recorded at import to select the embedded-preview path.
            let image = if asset
                .format
                .as_deref()
                .is_some_and(super::scan::is_raw_format)
            {
                match icatalog_media::raw_preview(&bytes) {
                    Ok(b) => b,
                    Err(_) => {
                        failed += 1;
                        continue;
                    }
                }
            } else {
                bytes
            };
            let orientation = ctx
                .catalog()
                .asset_meta(asset.id)?
                .and_then(|m| m.orientation);
            let preview =
                match icatalog_media::thumbnail_jpeg_with_orientation(&image, 1920, orientation) {
                    Ok(p) => p,
                    Err(_) => {
                        failed += 1;
                        continue;
                    }
                };
            let hash = crate::previews::hash(&preview.bytes);
            crate::previews::store(ctx.catalog(), ctx.path(), &hash, &preview.bytes, &mode)?;
            ctx.catalog().store_preview(&icatalog_catalog::Preview {
                asset_id: asset.id,
                hash,
                width: preview.width,
                height: preview.height,
                source_hash,
            })?;
            generated += 1;
        }
        ctx.catalog().prune_unreferenced_preview_blobs()?;
        Ok(Outcome::text(format!("previews: generated {generated}; cached {cached}; unavailable, changed or unsupported {failed} (run update for changed media)")))
    }
}
