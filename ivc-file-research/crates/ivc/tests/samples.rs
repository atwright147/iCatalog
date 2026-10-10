use std::path::PathBuf;

const FILES: [&str; 16] = [
    "iView157-01",
    "iView157-02",
    "iViewMedia1.3-Win-s01.ivc",
    "iViewMedia2.7-Win-s01.ivc",
    "IVM264-s01.ivc",
    "iViewPro26-s01.ivc",
    "iViewPro26-s02.ivc",
    "iViewPro302-s01.ivc",
    "iViewPro302-s02.ivc",
    "iViewPro311.ivc",
    "Expression1-s01.ivc",
    "Expression1-s02.ivc",
    "Expression2-s01.ivc",
    "Expression2-s02.ivc",
    "PhaseOneMediaPro1.mpcatalog",
    "PhaseOneMediaPro2.mpcatalog",
];

fn load(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn all_samples_parse_consistently() {
    for name in FILES {
        let data = load(name);
        let cat = ivc::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        // (b) header count == items walked
        assert_eq!(cat.header.item_count as usize, cat.items.len(), "{name}");
        // (c) first chunk known
        let first = cat
            .chunks
            .first()
            .unwrap_or_else(|| panic!("{name}: no chunks"));
        assert!(first.is_known(), "{name}: first chunk {:?}", first.kind);
        assert!(
            cat.chunks.iter().all(|c| c.is_known()),
            "{name}: unknown chunk"
        );
        // (d) trailer
        assert_eq!(cat.trailer.magic, cat.header.magic, "{name}");
        // trailer offset == end of last item == first chunk offset
        assert_eq!(cat.trailer.chunk_start, first.offset as u64, "{name}");
        let items_end = cat.items.last().map_or(8, |i| i.offset + i.len);
        assert_eq!(items_end, first.offset, "{name}");
        // PRONOM-style magic check
        let want: &[u8; 4] = if name.ends_with(".mpcatalog") {
            b"030i"
        } else {
            b"025i"
        };
        assert_eq!(&cat.header.magic, want, "{name}");
        assert_eq!(&data[4..8], want, "{name}");
        // (e) content files expose names
        for it in &cat.items {
            assert!(
                !it.name.is_empty() && !it.path.is_empty(),
                "{name}: {:?}",
                it.path
            );
            assert!(it.width > 0 && it.height > 0, "{name}");
            // PICT v2 header op: 0x0011 0x02ff after picSize(2)+frame(8)
            assert_eq!(&it.pict[10..14], &[0x00, 0x11, 0x02, 0xff], "{name}");
            // (f) thumbnail
            match it.jpeg {
                Some(j) => {
                    assert_eq!(&j[..3], &[0xff, 0xd8, 0xff], "{name}");
                    assert_eq!(&j[j.len() - 2..], &[0xff, 0xd9], "{name}");
                }
                None => assert_eq!(name, "iView157-02", "only classic Mac samples lack JPEG"),
            }
        }
    }
}

#[test]
fn expected_item_names() {
    let names = |f: &str| -> Vec<String> {
        let data = load(f);
        ivc::parse(&data)
            .map(|c| c.items.into_iter().map(|i| i.name).collect())
            .unwrap_or_default()
    };
    assert_eq!(names("Expression2-s02.ivc"), ["ArrowR.tif"]);
    assert_eq!(names("Expression1-s02.ivc"), ["memory.jpg"]);
    let e2 = names("Expression2-s01.ivc");
    assert_eq!(e2.len(), 4);
    assert!(e2[0].starts_with("Winter"));
    for f in [
        "iView157-02",
        "iViewPro26-s02.ivc",
        "PhaseOneMediaPro1.mpcatalog",
    ] {
        let n = names(f);
        assert_eq!(n.len(), 5, "{f}");
        assert_eq!(n[0], "Green Apple.gif", "{f}");
    }
}

#[test]
fn truncation_never_panics() {
    for name in FILES {
        let data = load(name);
        let step = (data.len() / 97).max(1);
        let mut cut = 0;
        while cut < data.len() {
            assert!(ivc::parse(&data[..cut]).is_err(), "{name} cut {cut}");
            cut += step;
        }
    }
    assert!(ivc::parse(&[]).is_err());
    assert!(ivc::parse(b"\0\0\0\0xxxx\0\0\0\0\0\0\0\0").is_err());
}

#[test]
fn garbage_never_panics() {
    let mut data = load("Expression2-s01.ivc");
    for i in (0..data.len()).step_by(7) {
        let saved = data[i];
        data[i] = data[i].wrapping_add(0x5b);
        let _ = ivc::parse(&data);
        data[i] = saved;
    }
}
