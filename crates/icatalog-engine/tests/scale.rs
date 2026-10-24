//! Reproducible large-library workflow using generated images, never personal media.
#![allow(clippy::unwrap_used, clippy::panic)]
use icatalog_catalog::AssetQuery;
use icatalog_engine::{dispatch_guarded, Context, Outcome, Params};
use std::time::Instant;

fn metric(outcome: &Outcome, name: &str) -> u64 {
    let Outcome::Table { rows, .. } = outcome else {
        panic!("expected metrics")
    };
    rows.iter().find(|row| row[0] == name).unwrap()[1]
        .parse()
        .unwrap()
}

#[test]
#[ignore = "10,000 generated images; run task scale"]
fn large_catalogue_workflow() {
    let base = std::env::temp_dir().join(format!("icatalog-scale-{}", std::process::id()));
    std::fs::create_dir(&base).unwrap();
    let images = base.join("images");
    std::fs::create_dir(&images).unwrap();
    let started = Instant::now();
    for index in 0..10_000u32 {
        let folder = images.join(format!("set-{:03}", index / 100));
        std::fs::create_dir_all(&folder).unwrap();
        let image = image::RgbImage::from_pixel(
            8,
            8,
            image::Rgb([(index & 255) as u8, ((index >> 8) & 255) as u8, 77]),
        );
        image
            .save(folder.join(format!("image-{index:05}.png")))
            .unwrap();
    }
    eprintln!("generate 10,000 images: {:?}", started.elapsed());
    let path = base.join("catalogue.sqlite");
    let mut context = Context::open(&path).unwrap();
    dispatch_guarded(&mut context, "init", &Params::new()).unwrap();
    let mut params = Params::new();
    params.set("path", images.to_str().unwrap());
    let started = Instant::now();
    let result = dispatch_guarded(&mut context, "import", &params).unwrap();
    assert_eq!(metric(&result, "imported"), 10_000);
    assert_eq!(context.catalog().stats().unwrap().thumbnails, 10_000);
    eprintln!("import with thumbnails: {:?}", started.elapsed());

    let started = Instant::now();
    let assets = context
        .catalog()
        .query_assets(&AssetQuery::default())
        .unwrap();
    assert_eq!(assets.len(), 10_000);
    eprintln!("browse 10,000 rows: {:?}", started.elapsed());
    let selected = assets
        .iter()
        .find(|asset| asset.name == "image-05000.png")
        .unwrap();
    let uid = selected.uid.clone();
    let mut edit = Params::new();
    edit.set("id", selected.id.to_string())
        .set("rating", "5")
        .set("keyword", "synthetic-favourite");
    dispatch_guarded(&mut context, "edit", &edit).unwrap();
    let mut find = Params::new();
    find.set("query", "image-05000")
        .set("keyword", "synthetic-favourite")
        .set("rating", "5");
    let started = Instant::now();
    let Outcome::Table { rows, .. } = dispatch_guarded(&mut context, "find", &find).unwrap() else {
        panic!("expected assets")
    };
    assert_eq!(rows.len(), 1);
    eprintln!("filtered search: {:?}", started.elapsed());

    let started = Instant::now();
    let result = dispatch_guarded(&mut context, "update", &Params::new()).unwrap();
    assert_eq!(metric(&result, "unchanged"), 10_000);
    eprintln!("unchanged rescan: {:?}", started.elapsed());
    std::fs::remove_file(images.join("set-000/image-00000.png")).unwrap();
    image::RgbImage::from_pixel(9, 9, image::Rgb([4, 5, 6]))
        .save(images.join("set-050/image-05000.png"))
        .unwrap();
    image::RgbImage::from_pixel(9, 9, image::Rgb([7, 8, 9]))
        .save(images.join("set-000/new.png"))
        .unwrap();
    let started = Instant::now();
    let result = dispatch_guarded(&mut context, "update", &Params::new()).unwrap();
    assert_eq!(metric(&result, "added"), 1);
    assert_eq!(metric(&result, "changed"), 1);
    assert_eq!(metric(&result, "missing"), 1);
    assert_eq!(
        context
            .catalog()
            .annotation(selected.id)
            .unwrap()
            .unwrap()
            .rating,
        5
    );
    assert_eq!(
        context.catalog().asset(selected.id).unwrap().unwrap().uid,
        uid
    );
    eprintln!("rescan added/changed/missing: {:?}", started.elapsed());
    drop(context);
    let mut reopened = Context::open(&path).unwrap();
    dispatch_guarded(&mut reopened, "open", &Params::new()).unwrap();
    assert_eq!(reopened.catalog().stats().unwrap().assets, 10_001);
    assert_eq!(
        reopened
            .catalog()
            .connection()
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(reopened);
    std::fs::remove_dir_all(&base).unwrap();
}
