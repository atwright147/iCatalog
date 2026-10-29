#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use icatalog_catalog::AssetQuery;
use icatalog_engine::*;

const XMP: &str = r#"<?xpacket begin=""?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:xmp="x" xmlns:dc="d">
<xmp:Rating>3</xmp:Rating><xmp:Label>Red</xmp:Label>
<dc:subject><rdf:Bag><rdf:li>cat</rdf:li><rdf:li>dog</rdf:li></rdf:Bag></dc:subject>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">Hello</rdf:li></rdf:Alt></dc:description>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

fn jpeg(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([seed, 10, 200]));
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut out)
        .encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// JPEG with an APP1 XMP segment inserted after SOI.
fn jpeg_xmp(seed: u8) -> Vec<u8> {
    let j = jpeg(24, 16, seed);
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(XMP.as_bytes());
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&j[2..]);
    out
}

/// Little-endian TIFF: IFD with preview (0x0201/0x0202) and optional XMP (700).
fn dng(preview: Option<&[u8]>, xmp: Option<&str>) -> Vec<u8> {
    let mut entries: Vec<(u16, u16, u32, u32)> = Vec::new();
    let n = u32::from(preview.is_some()) * 2 + u32::from(xmp.is_some()) + 1;
    let sub_off = 8 + 2 + 12 * n + 4;
    // SubIFD: full-resolution image 6048x4024 (NewSubfileType 0).
    let mut data: Vec<u8> = Vec::new();
    data.extend_from_slice(&3u16.to_le_bytes());
    for (tag, val) in [(254u16, 0u32), (256, 6048), (257, 4024)] {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&4u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&val.to_le_bytes());
    }
    data.extend_from_slice(&0u32.to_le_bytes());
    entries.push((0x014A, 4, 1, sub_off));
    let mut data_off = sub_off + data.len() as u32;
    if let Some(p) = preview {
        entries.push((0x0201, 4, 1, data_off));
        entries.push((0x0202, 4, 1, p.len() as u32));
        data.extend_from_slice(p);
        data_off += p.len() as u32;
    }
    if let Some(x) = xmp {
        entries.push((700, 1, x.len() as u32, data_off));
        data.extend_from_slice(x.as_bytes());
    }
    entries.sort_by_key(|e| e.0);
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, val) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn setup(name: &str) -> (Context, std::path::PathBuf, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("icat-rawx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("scan");
    std::fs::create_dir_all(&dir).unwrap();
    let mut ctx = Context::open(base.join("cat.sqlite")).unwrap();
    dispatch(&mut ctx, "init", &Params::new()).unwrap();
    (ctx, dir, base)
}

fn go(ctx: &mut Context, cmd: &str, dir: &std::path::Path) -> Outcome {
    let mut p = Params::new();
    p.set("path", dir.to_str().unwrap());
    dispatch(ctx, cmd, &p).unwrap()
}

fn metric(o: &Outcome, k: &str) -> u64 {
    match o {
        Outcome::Table { rows, .. } => rows
            .iter()
            .find(|r| r[0] == k)
            .map(|r| r[1].parse().unwrap())
            .unwrap_or(0),
        Outcome::Text(t) => panic!("{t}"),
    }
}

fn only_asset(ctx: &Context) -> i64 {
    let v = ctx.catalog().query_assets(&AssetQuery::default()).unwrap();
    assert_eq!(v.len(), 1);
    v[0].id
}

fn kws(ctx: &Context, id: i64) -> Vec<String> {
    let mut v: Vec<String> = ctx
        .catalog()
        .asset_keywords(id)
        .unwrap()
        .into_iter()
        .map(|k| k.name)
        .collect();
    v.sort();
    v
}

fn xmp_rows(ctx: &Context, id: i64) -> usize {
    ctx.catalog()
        .metadata_raw_for_asset(id)
        .unwrap()
        .iter()
        .filter(|m| m.scheme == "xmp_packet")
        .count()
}

#[test]
fn smart_rating_uses_imported_xmp_and_combines_with_other_rules() {
    let (mut ctx, dir, base) = setup("smart-rating");
    std::fs::write(dir.join("red_xmp.jpg"), jpeg_xmp(7)).unwrap();
    go(&mut ctx, "import", &dir);
    let id = only_asset(&ctx);
    assert_eq!(ctx.catalog().annotation(id).unwrap().unwrap().rating, 3);
    let mut params = Params::new();
    params
        .set("action", "create-smart")
        .set("name", "Rated")
        .set("min-rating", "3");
    dispatch(&mut ctx, "collection", &params).unwrap();
    let collection = ctx.catalog().all_collections().unwrap().remove(0).id;
    let query = AssetQuery {
        collection_id: Some(collection),
        ..Default::default()
    };
    assert_eq!(ctx.catalog().query_assets(&query).unwrap().len(), 1);
    assert_eq!(
        ctx.catalog().collection_assets(collection).unwrap(),
        vec![id]
    );
    // A minimum above the imported rating excludes it.
    let mut params = Params::new();
    params
        .set("action", "set-rules")
        .set("id", collection.to_string())
        .set("min-rating", "4");
    dispatch(&mut ctx, "collection", &params).unwrap();
    assert!(ctx.catalog().query_assets(&query).unwrap().is_empty());
    // Meeting the rating does not bypass a camera rule when EXIF is absent.
    params.set("min-rating", "3").set("camera", "Sony");
    dispatch(&mut ctx, "collection", &params).unwrap();
    assert!(ctx.catalog().query_assets(&query).unwrap().is_empty());
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn raw_preview_and_xmp_seeded() {
    let (mut ctx, dir, base) = setup("raw");
    std::fs::write(dir.join("a.dng"), dng(Some(&jpeg(32, 24, 5)), Some(XMP))).unwrap();
    let o = go(&mut ctx, "import", &dir);
    assert_eq!(metric(&o, "imported"), 1);
    let id = only_asset(&ctx);
    let t = ctx
        .catalog()
        .thumbnail(id, i64::from(icatalog_media::GRID_LONG_EDGE))
        .unwrap()
        .unwrap();
    assert_eq!(t.source, "raw_preview");
    let asset = ctx.catalog().query_assets(&AssetQuery::default()).unwrap();
    assert_eq!(asset[0].format.as_deref(), Some("dng"));
    assert_eq!((asset[0].width, asset[0].height), (Some(6048), Some(4024)));
    assert_eq!(xmp_rows(&ctx, id), 1);
    let a = ctx.catalog().annotation(id).unwrap().unwrap();
    assert_eq!(a.rating, 3);
    assert_eq!(a.label, 1); // Red maps to label 1
    assert_eq!(a.caption.as_deref(), Some("Hello"));
    assert_eq!(kws(&ctx, id), ["cat", "dog"]);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn raw_unknown_dimensions_are_null_never_preview() {
    let (mut ctx, dir, base) = setup("rafdim");
    // RAF: raw_dimensions() is None, but an embedded JPEG preview is found.
    let mut f = b"FUJIFILMCCD-RAW ".to_vec();
    f.extend_from_slice(&[0u8; 40]);
    f.extend_from_slice(&jpeg(32, 24, 5));
    f.extend_from_slice(&[0u8; 10]);
    std::fs::write(dir.join("a.raf"), &f).unwrap();
    let o = go(&mut ctx, "import", &dir);
    assert_eq!(metric(&o, "imported"), 1);
    let id = only_asset(&ctx);
    let asset = ctx.catalog().query_assets(&AssetQuery::default()).unwrap();
    assert_eq!(asset[0].format.as_deref(), Some("raf"));
    assert_eq!(asset[0].width, None);
    assert_eq!(asset[0].height, None);
    let t = ctx
        .catalog()
        .thumbnail(id, i64::from(icatalog_media::GRID_LONG_EDGE))
        .unwrap()
        .unwrap();
    assert_eq!(t.source, "raw_preview");
    assert!(t.width > 0 && t.height > 0);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn jpeg_xmp_stored_and_seeded() {
    let (mut ctx, dir, base) = setup("jpg");
    std::fs::write(dir.join("a.jpg"), jpeg_xmp(7)).unwrap();
    go(&mut ctx, "import", &dir);
    let id = only_asset(&ctx);
    let t = ctx
        .catalog()
        .thumbnail(id, i64::from(icatalog_media::GRID_LONG_EDGE))
        .unwrap()
        .unwrap();
    assert_eq!(t.source, "rendered");
    let asset = ctx.catalog().query_assets(&AssetQuery::default()).unwrap();
    assert_eq!(asset[0].format.as_deref(), Some("jpeg"));
    assert_eq!((asset[0].width, asset[0].height), (Some(24), Some(16)));
    assert_eq!(xmp_rows(&ctx, id), 1);
    assert_eq!(ctx.catalog().annotation(id).unwrap().unwrap().rating, 3);
    assert_eq!(kws(&ctx, id), ["cat", "dog"]);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn reimport_is_idempotent() {
    let (mut ctx, dir, base) = setup("dup");
    std::fs::write(dir.join("a.jpg"), jpeg_xmp(7)).unwrap();
    go(&mut ctx, "import", &dir);
    let id = only_asset(&ctx);
    let mut p = Params::new();
    p.set("id", id.to_string());
    p.set("rating", "5");
    dispatch(&mut ctx, "edit", &p).unwrap();
    let o = go(&mut ctx, "import", &dir);
    assert_eq!(metric(&o, "duplicates"), 1);
    assert_eq!(xmp_rows(&ctx, id), 1);
    assert_eq!(ctx.catalog().annotation(id).unwrap().unwrap().rating, 5);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn edit_survives_update() {
    let (mut ctx, dir, base) = setup("upd");
    let f = dir.join("a.dng");
    std::fs::write(&f, dng(Some(&jpeg(32, 24, 5)), Some(XMP))).unwrap();
    go(&mut ctx, "import", &dir);
    let id = only_asset(&ctx);
    let mut p = Params::new();
    p.set("id", id.to_string());
    p.set("rating", "4");
    p.set("keyword", "foo");
    dispatch(&mut ctx, "edit", &p).unwrap();
    // Change the file so update takes the Changed path.
    std::fs::write(&f, dng(Some(&jpeg(40, 30, 99)), Some(XMP))).unwrap();
    go(&mut ctx, "update", &dir);
    assert_eq!(ctx.catalog().annotation(id).unwrap().unwrap().rating, 4);
    assert!(kws(&ctx, id).contains(&"foo".to_string()));
    assert_eq!(xmp_rows(&ctx, id), 1);
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn raw_without_preview_skipped() {
    let (mut ctx, dir, base) = setup("nop");
    std::fs::write(dir.join("a.dng"), dng(None, Some(XMP))).unwrap();
    let o = go(&mut ctx, "import", &dir);
    assert_eq!(metric(&o, "imported"), 0);
    assert_eq!(metric(&o, "skipped: RAW file with no usable preview"), 1);
    assert!(ctx
        .catalog()
        .query_assets(&AssetQuery::default())
        .unwrap()
        .is_empty());
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn plain_tiff_uses_rendered_path() {
    let (mut ctx, dir, base) = setup("tif");
    let img = image::RgbImage::from_pixel(20, 10, image::Rgb([1, 2, 3]));
    let mut cur = std::io::Cursor::new(Vec::new());
    image::ImageEncoder::write_image(
        image::codecs::tiff::TiffEncoder::new(&mut cur),
        img.as_raw(),
        20,
        10,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let out = cur.into_inner();
    std::fs::write(dir.join("a.tif"), &out).unwrap();
    let o = go(&mut ctx, "import", &dir);
    assert_eq!(metric(&o, "imported"), 1);
    let id = only_asset(&ctx);
    let t = ctx
        .catalog()
        .thumbnail(id, i64::from(icatalog_media::GRID_LONG_EDGE))
        .unwrap()
        .unwrap();
    assert_eq!(t.source, "rendered");
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn hostile_raw_never_panics() {
    let (mut ctx, dir, base) = setup("bad");
    let good = dng(Some(&jpeg(32, 24, 5)), Some(XMP));
    for (i, n) in [0usize, 3, 4, 9, 20, 40, good.len() / 2].iter().enumerate() {
        std::fs::write(dir.join(format!("t{i}.dng")), &good[..*n]).unwrap();
    }
    std::fs::write(dir.join("g.cr3"), b"II*\0\xff\xff\xff\xffgarbage").unwrap();
    std::fs::write(dir.join("h.nef"), vec![0xAB; 5000]).unwrap();
    let mut p = Params::new();
    p.set("path", dir.to_str().unwrap());
    let _ = dispatch(&mut ctx, "import", &p);
    // Catalogue remains usable.
    dispatch(&mut ctx, "list", &Params::new()).unwrap();
    drop(ctx);
    let _ = std::fs::remove_dir_all(&base);
}
