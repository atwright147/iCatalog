#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_catalog::AssetQuery;
use icatalog_engine::{dispatch, Context, Params};

fn jpeg(w: u32, h: u32, orientation: Option<u16>) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([80, 120, 160]));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&img)
        .unwrap();
    let Some(orientation) = orientation else {
        return bytes;
    };
    let mut exif = b"Exif\0\0II*\0\x08\0\0\0".to_vec();
    exif.extend_from_slice(&1u16.to_le_bytes());
    exif.extend_from_slice(&0x0112u16.to_le_bytes());
    exif.extend_from_slice(&3u16.to_le_bytes());
    exif.extend_from_slice(&1u32.to_le_bytes());
    exif.extend_from_slice(&u32::from(orientation).to_le_bytes());
    exif.extend_from_slice(&0u32.to_le_bytes());
    let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
    out.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&exif);
    out.extend_from_slice(&bytes[2..]);
    out
}

fn setup(name: &str) -> (Context, std::path::PathBuf, Params) {
    let base = std::env::temp_dir().join(format!("icat-orientation-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("scan");
    std::fs::create_dir_all(&dir).unwrap();
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    let mut params = Params::new();
    params.set("path", dir.to_str().unwrap());
    (ctx, base, params)
}

#[test]
fn import_and_update_store_oriented_pixels_and_clear_lost_exif() {
    let (mut ctx, base, params) = setup("jpeg");
    let path = base.join("scan/photo.jpg");
    std::fs::write(&path, jpeg(96, 64, Some(6))).unwrap();
    dispatch(&mut ctx, "import", &params).unwrap();
    let original = ctx
        .catalog()
        .query_assets(&AssetQuery::default())
        .unwrap()
        .remove(0);
    // Reopen to prove the pixels, flag and metadata survive persistence.
    drop(ctx);
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    for (w, h, orientation, expected) in [
        (96, 64, Some(6), (64, 96)),
        (120, 80, Some(8), (80, 120)),
        (100, 60, None, (100, 60)),
        (80, 40, Some(0), (80, 40)),
    ] {
        if w != 96 {
            std::fs::write(&path, jpeg(w, h, orientation)).unwrap();
            dispatch(&mut ctx, "update", &params).unwrap();
        }
        let asset = ctx.catalog().asset(original.id).unwrap().unwrap();
        assert_eq!(asset.uid, original.uid);
        assert_eq!(
            (asset.width, asset.height),
            (Some(i64::from(w)), Some(i64::from(h)))
        );
        assert_eq!(
            ctx.catalog()
                .asset_meta(asset.id)
                .unwrap()
                .unwrap()
                .orientation,
            orientation.map(i64::from)
        );
        let thumb = ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap();
        assert_eq!((thumb.width, thumb.height), expected);
        assert_eq!(
            thumb.orientation_applied,
            matches!(orientation, Some(1..=8))
        );
        assert_eq!(thumb.generator_version, icatalog_media::GENERATOR_VERSION);
        let pixels = icatalog_media::decode_rgba(&thumb.bytes).unwrap();
        assert_eq!(
            (i64::from(pixels.width), i64::from(pixels.height)),
            expected
        );
    }
    drop(ctx);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn update_with_malformed_exif_clears_stale_orientation() {
    let (mut ctx, base, params) = setup("malformed");
    let path = base.join("scan/photo.jpg");
    std::fs::write(&path, jpeg(96, 64, Some(6))).unwrap();
    dispatch(&mut ctx, "import", &params).unwrap();
    let id = ctx.catalog().query_assets(&AssetQuery::default()).unwrap()[0].id;
    let mut changed = jpeg(100, 70, Some(6));
    // Damage the TIFF header inside APP1 while keeping JPEG pixels decodable.
    changed[12..16].copy_from_slice(b"bad!");
    assert!(icatalog_media::read_exif(std::io::Cursor::new(&changed)).is_err());
    std::fs::write(path, changed).unwrap();
    dispatch(&mut ctx, "update", &params).unwrap();
    assert_eq!(
        ctx.catalog().asset_meta(id).unwrap().unwrap().orientation,
        None
    );
    let thumb = ctx.catalog().thumbnail(id, 256).unwrap().unwrap();
    assert!(!thumb.orientation_applied);
    assert_eq!((thumb.width, thumb.height), (100, 70));
    drop(ctx);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn raw_thumbnail_uses_the_selected_asset_metadata_once() {
    let (mut ctx, base, params) = setup("raw");
    // The RAW's orientation is authoritative; do not also rotate by the
    // embedded JPEG's own orientation. No source file pixels are rewritten.
    let preview = jpeg(96, 64, Some(8));
    let mut raw = b"II*\0\x08\0\0\0".to_vec();
    raw.extend_from_slice(&3u16.to_le_bytes());
    for (tag, ty, value) in [
        (0x0112u16, 3u16, 6u32),
        (0x0201, 4, 50),
        (0x0202, 4, preview.len() as u32),
    ] {
        raw.extend_from_slice(&tag.to_le_bytes());
        raw.extend_from_slice(&ty.to_le_bytes());
        raw.extend_from_slice(&1u32.to_le_bytes());
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend_from_slice(&0u32.to_le_bytes());
    raw.extend_from_slice(&preview);
    let path = base.join("scan/photo.dng");
    std::fs::write(&path, &raw).unwrap();
    dispatch(&mut ctx, "import", &params).unwrap();
    let asset = ctx
        .catalog()
        .query_assets(&AssetQuery::default())
        .unwrap()
        .remove(0);
    assert_eq!(
        ctx.catalog()
            .asset_meta(asset.id)
            .unwrap()
            .unwrap()
            .orientation,
        Some(6)
    );
    let thumb = ctx.catalog().thumbnail(asset.id, 256).unwrap().unwrap();
    assert_eq!(thumb.source, "raw_preview");
    assert!(thumb.orientation_applied);
    assert_eq!((thumb.width, thumb.height), (64, 96));
    assert_eq!(std::fs::read(path).unwrap(), raw);
    drop(ctx);
    let _ = std::fs::remove_dir_all(base);
}
