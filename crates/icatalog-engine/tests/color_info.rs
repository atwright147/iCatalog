#![allow(clippy::unwrap_used)]
use icatalog_catalog::AssetQuery;
use icatalog_engine::{dispatch, Context, Params};
use image::ImageEncoder;

fn jpeg(profile: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
    if profile {
        let mut icc = vec![0; 144];
        icc[16..20].copy_from_slice(b"RGB ");
        icc[36..40].copy_from_slice(b"acsp");
        icc[128..132].copy_from_slice(&1u32.to_be_bytes());
        icc[132..136].copy_from_slice(b"desc");
        icc[136..140].copy_from_slice(&144u32.to_be_bytes());
        let name = b"Synthetic RGB profile\0";
        icc[140..144].copy_from_slice(&(12u32 + name.len() as u32).to_be_bytes());
        icc.extend_from_slice(b"desc\0\0\0\0");
        icc.extend_from_slice(&(name.len() as u32).to_be_bytes());
        icc.extend_from_slice(name);
        let size = icc.len() as u32;
        icc[..4].copy_from_slice(&size.to_be_bytes());
        encoder.set_icc_profile(icc).unwrap();
    }
    encoder
        .encode(&[42; 24], 4, 2, image::ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

#[test]
fn import_update_backfill_and_reopen_source_colour_information() {
    let base = std::env::temp_dir().join(format!("icat-color-{}", std::process::id()));
    std::fs::create_dir_all(base.join("scan")).unwrap();
    let media = base.join("scan/a.jpg");
    std::fs::write(&media, jpeg(true)).unwrap();
    let path = base.join("catalogue.sqlite");
    let mut ctx = Context::open(&path).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    let mut params = Params::new();
    params.set("path", base.join("scan").to_string_lossy());
    dispatch(&mut ctx, "import", &params).unwrap();
    let asset = ctx
        .catalog()
        .query_assets(&AssetQuery::default())
        .unwrap()
        .remove(0);
    let read = |ctx: &Context, key: &str| {
        ctx.catalog()
            .metadata_raw_for_asset_scheme(asset.id, "image_info")
            .unwrap()
            .into_iter()
            .find(|m| m.key == key)
            .unwrap()
            .value
    };
    assert_eq!(read(&ctx, "color_model"), b"RGB");
    assert_eq!(read(&ctx, "icc_profile"), b"Synthetic RGB profile");
    let thumbnail = ctx.catalog().thumbnail(asset.id, 256).unwrap();
    ctx.catalog()
        .connection()
        .execute(
            "DELETE FROM metadata_raw WHERE asset_id=?1 AND scheme='image_info'",
            [asset.id],
        )
        .unwrap();
    dispatch(&mut ctx, "update", &params).unwrap();
    assert_eq!(read(&ctx, "icc_profile"), b"Synthetic RGB profile");
    assert_eq!(ctx.catalog().thumbnail(asset.id, 256).unwrap(), thumbnail);
    assert_eq!(
        ctx.catalog().asset(asset.id).unwrap().unwrap().uid,
        asset.uid
    );
    // The new source has no profile and a different sample model. No stale ICC name survives.
    let mut gray = Vec::new();
    image::codecs::png::PngEncoder::new(&mut gray)
        .write_image(&[10; 24], 6, 4, image::ExtendedColorType::L8)
        .unwrap();
    std::fs::write(&media, gray).unwrap();
    dispatch(&mut ctx, "update", &params).unwrap();
    assert_eq!(read(&ctx, "color_model"), b"Gray");
    assert!(read(&ctx, "icc_profile").is_empty());
    drop(ctx);
    let reopened = Context::open(&path).unwrap();
    assert_eq!(read(&reopened, "color_model"), b"Gray");
    assert!(read(&reopened, "icc_profile").is_empty());
    assert!(reopened
        .catalog()
        .metadata_raw_for_asset_scheme(asset.id, "other")
        .unwrap()
        .is_empty());
    drop(reopened);
    std::fs::remove_dir_all(base).unwrap();
}
