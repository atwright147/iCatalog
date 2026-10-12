//! Locate and tolerantly parse embedded XMP metadata (no XML crate).
//!
//! All parsing is bounds-checked: hostile input yields `None`/empty, never a panic.

const MAX_PACKET: usize = 4 * 1024 * 1024;
const MAX_KEYWORDS: usize = 10_000;
const MAX_STRING: usize = 64 * 1024;
const MAX_DEPTH: usize = 256;
const MAX_SEGMENTS: usize = 4096;
const MAX_CHUNKS: usize = 4096;
const MAX_IFDS: usize = 64;
const MAX_ENTRIES: usize = 4096;
const XAP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/";
const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Fields extracted from an XMP packet. Missing fields are None / empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XmpData {
    /// `xmp:Rating`, only kept if 0..=5.
    pub rating: Option<u8>,
    /// `xmp:Label` (a colour-name string; not mapped here).
    pub label: Option<String>,
    /// `dc:subject` items.
    pub keywords: Vec<String>,
    /// `dc:description`, preferring `x-default`.
    pub description: Option<String>,
}

fn lossy(b: &[u8]) -> String {
    let b = b.get(..MAX_PACKET).unwrap_or(b);
    String::from_utf8_lossy(b).into_owned()
}

/// Locate an embedded XMP packet and return it as lossy UTF-8.
pub fn xmp_packet(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg_packet(bytes)
    } else if bytes.starts_with(PNG_SIG) {
        png_packet(bytes)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        tiff_packet(bytes)
    } else {
        None
    }
}

fn jpeg_packet(b: &[u8]) -> Option<String> {
    let mut pos = 2usize;
    for _ in 0..MAX_SEGMENTS {
        // Skip fill bytes.
        while b.get(pos) == Some(&0xFF) && b.get(pos.checked_add(1)?) == Some(&0xFF) {
            pos = pos.checked_add(1)?;
        }
        if *b.get(pos)? != 0xFF {
            return None;
        }
        let marker = *b.get(pos.checked_add(1)?)?;
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        pos = pos.checked_add(2)?;
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        let len = usize::from(u16::from_be_bytes([
            *b.get(pos)?,
            *b.get(pos.checked_add(1)?)?,
        ]));
        let data_start = pos.checked_add(2)?;
        let end = pos.checked_add(len)?;
        if len < 2 {
            return None;
        }
        if marker == 0xE1 {
            let seg = b.get(data_start..end)?;
            if let Some(rest) = seg.strip_prefix(XAP_NS) {
                if let Some(rest) = rest.strip_prefix(&[0u8][..]) {
                    return Some(lossy(rest));
                }
            }
        }
        pos = end;
    }
    None
}

fn png_packet(b: &[u8]) -> Option<String> {
    let mut pos = PNG_SIG.len();
    for _ in 0..MAX_CHUNKS {
        let lb: [u8; 4] = b.get(pos..pos.checked_add(4)?)?.try_into().ok()?;
        let len = usize::try_from(u32::from_be_bytes(lb)).ok()?;
        let ty = b.get(pos.checked_add(4)?..pos.checked_add(8)?)?;
        let ds = pos.checked_add(8)?;
        let de = ds.checked_add(len)?;
        if ty == b"iTXt" {
            let data = b.get(ds..de)?;
            if let Some(text) = itxt_xmp(data) {
                return Some(text);
            }
        }
        if ty == b"IEND" {
            return None;
        }
        pos = de.checked_add(4)?;
    }
    None
}

fn itxt_xmp(data: &[u8]) -> Option<String> {
    let k = data.iter().position(|&c| c == 0)?;
    if data.get(..k)? != b"XML:com.adobe.xmp" {
        return None;
    }
    let flag = *data.get(k.checked_add(1)?)?;
    let mut p = k.checked_add(3)?; // skip flag + method
    for _ in 0..2 {
        // language tag, translated keyword
        let rel = data.get(p..)?.iter().position(|&c| c == 0)?;
        p = p.checked_add(rel)?.checked_add(1)?;
    }
    if flag != 0 {
        return None;
    }
    Some(lossy(data.get(p..)?))
}

struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        let a: [u8; 2] = self.b.get(o..o.checked_add(2)?)?.try_into().ok()?;
        Some(if self.le {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let a: [u8; 4] = self.b.get(o..o.checked_add(4)?)?.try_into().ok()?;
        Some(if self.le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }
    fn off(&self, o: usize) -> Option<usize> {
        usize::try_from(self.u32(o)?).ok()
    }
}

fn tiff_packet(b: &[u8]) -> Option<String> {
    let t = Tiff {
        b,
        le: b.starts_with(b"II"),
    };
    let mut queue: Vec<usize> = vec![t.off(4)?];
    let mut seen: Vec<usize> = Vec::new();
    while let Some(ifd) = queue.pop() {
        if seen.len() >= MAX_IFDS || seen.contains(&ifd) || ifd == 0 {
            continue;
        }
        seen.push(ifd);
        let Some(n) = t.u16(ifd) else { continue };
        let n = usize::from(n).min(MAX_ENTRIES);
        for i in 0..n {
            let Some(e) = i
                .checked_mul(12)
                .and_then(|x| x.checked_add(ifd))
                .and_then(|x| x.checked_add(2))
            else {
                break;
            };
            let (Some(tag), Some(ty), Some(count), Some(val)) = (
                t.u16(e),
                e.checked_add(2).and_then(|x| t.u16(x)),
                e.checked_add(4).and_then(|x| t.off(x)),
                e.checked_add(8),
            ) else {
                break;
            };
            if tag == 700 && (ty == 1 || ty == 7) {
                let off = if count <= 4 { Some(val) } else { t.off(val) };
                let packet = off
                    .and_then(|o| o.checked_add(count).map(|end| (o, end)))
                    .and_then(|(o, end)| b.get(o..end));
                if let Some(p) = packet {
                    return Some(lossy(p));
                }
            }
            if tag == 330 && ty == 4 && count > 0 && count <= 16 {
                if count == 1 {
                    if let Some(o) = t.off(val) {
                        queue.push(o);
                    }
                } else if let Some(base) = t.off(val) {
                    for j in 0..count {
                        let o = j.checked_mul(4).and_then(|x| x.checked_add(base));
                        if let Some(v) = o.and_then(|o| t.off(o)) {
                            queue.push(v);
                        }
                    }
                }
            }
        }
        let next = n
            .checked_mul(12)
            .and_then(|x| x.checked_add(ifd))
            .and_then(|x| x.checked_add(2));
        if let Some(nx) = next.and_then(|x| t.off(x)) {
            queue.push(nx);
        }
    }
    None
}

fn decode_entities(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let after = rest.get(i + 1..).unwrap_or("");
        let semi = after.find(';').filter(|&e| e <= 10);
        let decoded = semi.and_then(|e| {
            let name = after.get(..e)?;
            let ch = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let num = name.strip_prefix('#')?;
                    let v = match num.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => num.parse::<u32>().ok()?,
                    };
                    char::from_u32(v)?
                }
            };
            Some((ch, e))
        });
        match decoded {
            Some((ch, e)) => {
                out.push(ch);
                rest = after.get(e + 1..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let mut out = String::new();
    for c in t.chars() {
        if out.len() + c.len_utf8() > MAX_STRING {
            break;
        }
        out.push(c);
    }
    Some(out)
}

/// Lowercased qualified name (`prefix:local`), as written.
fn qname(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// Local part of a (possibly qualified) lowercased name.
fn local_of(full: &str) -> &str {
    full.rsplit(':').next().unwrap_or(full)
}

/// True if `full` has local name `want` and is not in the `rdf` namespace.
fn ctx_kind(full: &str, want: &str) -> bool {
    match full.split_once(':') {
        Some((prefix, local)) => local == want && !prefix.eq_ignore_ascii_case("rdf"),
        None => full == want,
    }
}

/// Parse attributes from a tag body (after the name). Returns (local name, value).
fn attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while out.len() < 64 {
        let Some(eq) = rest.find('=') else { break };
        let name = rest.get(..eq).unwrap_or("").trim();
        let after = rest.get(eq + 1..).unwrap_or("").trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            rest = after;
            continue;
        };
        let body = after.get(1..).unwrap_or("");
        let Some(end) = body.find(q) else { break };
        let value = decode_entities(body.get(..end).unwrap_or(""));
        let name = name.rsplit(char::is_whitespace).next().unwrap_or(name);
        out.push((qname(name), value));
        rest = body.get(end + 1..).unwrap_or("");
    }
    out
}

struct Frame {
    name: String,
    text: String,
    lang: Option<String>,
}

#[derive(Default)]
struct Acc {
    data: XmpData,
    desc_default: Option<String>,
    desc_first: Option<String>,
    desc_plain: Option<String>,
}

fn push_text(stack: &mut [Frame], s: &str) {
    if let Some(f) = stack.last_mut() {
        if f.text.len() < MAX_STRING * 2 {
            f.text.push_str(s);
        }
    }
}

/// Tolerantly parse the supported subset from an XMP packet. Never panics.
pub fn parse_xmp(xml: &str) -> XmpData {
    let xml = if xml.len() > MAX_PACKET {
        let mut end = MAX_PACKET;
        while !xml.is_char_boundary(end) {
            end -= 1;
        }
        xml.get(..end).unwrap_or("")
    } else {
        xml
    };
    let mut acc = Acc::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut rest = xml;

    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            push_text(&mut stack, &decode_entities(rest));
            break;
        };
        if lt > 0 {
            push_text(&mut stack, &decode_entities(rest.get(..lt).unwrap_or("")));
        }
        rest = rest.get(lt..).unwrap_or("");
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").and_then(|e| r.get(e + 3..)).unwrap_or("");
            continue;
        }
        if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let (txt, next) = match r.find("]]>") {
                Some(e) => (r.get(..e).unwrap_or(""), r.get(e + 3..).unwrap_or("")),
                None => (r, ""),
            };
            push_text(&mut stack, txt);
            rest = next;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest.find('>').map_or(rest.len(), |e| e + 1);
            rest = rest.get(end..).unwrap_or("");
            continue;
        }
        // Tag: find closing '>' outside quotes.
        let body_all = rest.get(1..).unwrap_or("");
        let mut quote: Option<char> = None;
        let mut close = None;
        for (i, c) in body_all.char_indices() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => {
                    close = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let (tag, next) = match close {
            Some(i) => (
                body_all.get(..i).unwrap_or(""),
                body_all.get(i + 1..).unwrap_or(""),
            ),
            None => (body_all, ""),
        };
        rest = next;

        if let Some(name) = tag.strip_prefix('/') {
            let name = qname(name.trim());
            let want = local_of(&name);
            if let Some(pos) = stack.iter().rposition(|f| local_of(&f.name) == want) {
                // Close any unterminated children, then the element itself.
                while stack.len() > pos {
                    if let Some(f) = stack.pop() {
                        finish(&f, &stack, &mut acc);
                    }
                }
            }
            continue;
        }
        let selfclose = tag.ends_with('/');
        let tag = tag.strip_suffix('/').unwrap_or(tag);
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let name = qname(tag.get(..name_end).unwrap_or(""));
        let mut lang = None;
        for (k, v) in attrs(tag.get(name_end..).unwrap_or("")) {
            if ctx_kind(&k, "rating") {
                set_rating(&mut acc.data, &v);
            } else if ctx_kind(&k, "label") {
                if acc.data.label.is_none() {
                    acc.data.label = clean(&v);
                }
            } else if local_of(&k) == "lang" {
                lang = Some(v);
            }
        }
        let frame = Frame {
            name,
            text: String::new(),
            lang,
        };
        if selfclose {
            finish(&frame, &stack, &mut acc);
        } else if stack.len() < MAX_DEPTH {
            stack.push(frame);
        } else {
            break;
        }
    }
    // Close any unterminated elements best-effort.
    while let Some(f) = stack.pop() {
        finish(&f, &stack, &mut acc);
    }
    acc.data.description = acc.desc_default.or(acc.desc_first).or(acc.desc_plain);
    acc.data
}

fn set_rating(data: &mut XmpData, v: &str) {
    if data.rating.is_none() {
        data.rating = v
            .trim()
            .parse::<i64>()
            .ok()
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| *n <= 5);
    }
}

fn finish(f: &Frame, stack: &[Frame], acc: &mut Acc) {
    let in_ctx = |n: &str| stack.iter().any(|s| ctx_kind(&s.name, n));
    match local_of(&f.name) {
        "rating" if ctx_kind(&f.name, "rating") => set_rating(&mut acc.data, &f.text),
        "label" if ctx_kind(&f.name, "label") => {
            if acc.data.label.is_none() {
                acc.data.label = clean(&f.text);
            }
        }
        "li" => {
            let Some(text) = clean(&f.text) else { return };
            if in_ctx("subject") {
                if acc.data.keywords.len() < MAX_KEYWORDS {
                    acc.data.keywords.push(text);
                }
            } else if in_ctx("description") {
                let is_default = f
                    .lang
                    .as_deref()
                    .is_some_and(|l| l.eq_ignore_ascii_case("x-default"));
                if is_default {
                    acc.desc_default.get_or_insert(text);
                } else {
                    acc.desc_first.get_or_insert(text);
                }
            }
        }
        "description" if ctx_kind(&f.name, "description") => {
            // Plain-text form; rdf:Description elements normally have only whitespace.
            if let Some(t) = clean(&f.text) {
                acc.desc_plain.get_or_insert(t);
            }
        }
        _ => {}
    }
}

/// Locate then parse.
pub fn read_xmp(bytes: &[u8]) -> Option<XmpData> {
    xmp_packet(bytes).map(|p| parse_xmp(&p))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const ELEM: &str = r#"<?xpacket begin=""?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"><rdf:Description xmlns:xmp="x" xmlns:dc="d">
<xmp:Rating>5</xmp:Rating><xmp:Label>Red</xmp:Label>
<dc:subject><rdf:Bag><rdf:li>cat</rdf:li><rdf:li>dog</rdf:li></rdf:Bag></dc:subject>
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">Hello</rdf:li></rdf:Alt></dc:description>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

    #[test]
    fn element_form() {
        let d = parse_xmp(ELEM);
        assert_eq!(d.rating, Some(5));
        assert_eq!(d.label.as_deref(), Some("Red"));
        assert_eq!(d.keywords, ["cat", "dog"]);
        assert_eq!(d.description.as_deref(), Some("Hello"));
    }

    #[test]
    fn attribute_form() {
        let d = parse_xmp(r#"<rdf:Description xmp:Rating="4" xmp:Label='Red'/>"#);
        assert_eq!(d.rating, Some(4));
        assert_eq!(d.label.as_deref(), Some("Red"));
    }

    #[test]
    fn bag_and_seq() {
        let d = parse_xmp("<dc:subject><rdf:Seq><rdf:li>a</rdf:li><rdf:li>b</rdf:li></rdf:Seq></dc:subject><dc:subject><rdf:Bag><rdf:li>c</rdf:li></rdf:Bag></dc:subject>");
        assert_eq!(d.keywords, ["a", "b", "c"]);
    }

    #[test]
    fn alt_prefers_default() {
        let d = parse_xmp(
            r#"<dc:description><rdf:Alt><rdf:li xml:lang="de">Hallo</rdf:li><rdf:li xml:lang="x-default">Hi</rdf:li></rdf:Alt></dc:description>"#,
        );
        assert_eq!(d.description.as_deref(), Some("Hi"));
        let d = parse_xmp("<dc:description>plain</dc:description>");
        assert_eq!(d.description.as_deref(), Some("plain"));
    }

    #[test]
    fn crs_group_li_is_not_a_description() {
        let d = parse_xmp(
            r#"<rdf:Description rdf:about="" crs:Copyright="c"><crs:Group><rdf:Alt><rdf:li xml:lang="x-default">Profiles</rdf:li></rdf:Alt></crs:Group></rdf:Description>"#,
        );
        assert_eq!(d.description, None);
        assert!(d.keywords.is_empty());
    }

    #[test]
    fn dc_description_wins_over_decoy_and_subject_still_works() {
        let d = parse_xmp(
            r#"<rdf:Description a="b"><crs:Group><rdf:Alt><rdf:li xml:lang="x-default">Profiles</rdf:li></rdf:Alt></crs:Group><dc:description><rdf:Alt><rdf:li xml:lang="x-default">Real</rdf:li></rdf:Alt></dc:description><dc:subject><rdf:Bag><rdf:li>k</rdf:li></rdf:Bag></dc:subject></rdf:Description>"#,
        );
        assert_eq!(d.description.as_deref(), Some("Real"));
        assert_eq!(d.keywords, ["k"]);
    }

    #[test]
    fn entities() {
        let d = parse_xmp("<dc:subject><rdf:Bag><rdf:li>a&amp;b &#65;&#x42; &lt;&bogus;</rdf:li></rdf:Bag></dc:subject>");
        assert_eq!(d.keywords, ["a&b AB <&bogus;"]);
    }

    #[test]
    fn cdata_and_comments() {
        let d = parse_xmp("<!-- <xmp:Rating>1</xmp:Rating> --><xmp:Label><![CDATA[Blue <b>]]></xmp:Label><xmp:Rating>2</xmp:Rating>");
        assert_eq!(d.label.as_deref(), Some("Blue <b>"));
        assert_eq!(d.rating, Some(2));
    }

    #[test]
    fn missing_and_malformed() {
        assert_eq!(parse_xmp("<a>text</a>"), XmpData::default());
        assert_eq!(parse_xmp("<xmp:Rating>9</xmp:Rating>").rating, None);
        assert_eq!(parse_xmp("<xmp:Rating>-1</xmp:Rating>").rating, None);
        let d = parse_xmp("<xmp:Rating>3</xmp:Rating><dc:subject><rdf:li>x");
        assert_eq!(d.rating, Some(3));
        for s in [
            "<",
            "<a",
            "<a b=\"",
            "</",
            "<![CDATA[",
            "<!--",
            "&",
            "<?",
            "</x></y>",
        ] {
            let _ = parse_xmp(s);
        }
    }

    #[test]
    fn hostile() {
        let deep = "<a>".repeat(100_000);
        let _ = parse_xmp(&deep);
        let mut s = String::from("<dc:subject><rdf:Bag>");
        for _ in 0..20_000 {
            s.push_str("<rdf:li>k</rdf:li>");
        }
        assert_eq!(parse_xmp(&s).keywords.len(), MAX_KEYWORDS);
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let mut p = XAP_NS.to_vec();
        p.push(0);
        p.extend_from_slice(b"<xmp:Label>\xFF\xFEx</xmp:Label>");
        jpg.extend_from_slice(&u16::try_from(p.len() + 2).unwrap().to_be_bytes());
        jpg.extend_from_slice(&p);
        let pkt = xmp_packet(&jpg).unwrap();
        assert!(pkt.contains('\u{FFFD}'));
        assert!(read_xmp(&jpg).unwrap().label.is_some());
    }

    fn jpeg() -> Vec<u8> {
        let mut p = XAP_NS.to_vec();
        p.push(0);
        p.extend_from_slice(ELEM.as_bytes());
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xE1];
        j.extend_from_slice(&u16::try_from(p.len() + 2).unwrap().to_be_bytes());
        j.extend_from_slice(&p);
        j.extend_from_slice(&[0xFF, 0xD9]);
        j
    }

    fn png() -> Vec<u8> {
        let mut d = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        d.extend_from_slice(ELEM.as_bytes());
        let mut f = PNG_SIG.to_vec();
        f.extend_from_slice(&u32::try_from(d.len()).unwrap().to_be_bytes());
        f.extend_from_slice(b"iTXt");
        f.extend_from_slice(&d);
        f.extend_from_slice(&[0; 4]);
        f.extend_from_slice(&[0, 0, 0, 0]);
        f.extend_from_slice(b"IEND");
        f.extend_from_slice(&[0; 4]);
        f
    }

    fn tiff(le: bool) -> Vec<u8> {
        let x = ELEM.as_bytes();
        let u16b = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let u32b = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut t = Vec::new();
        t.extend_from_slice(if le { b"II*\0" } else { b"MM\0*" });
        t.extend_from_slice(&u32b(8));
        t.extend_from_slice(&u16b(1));
        t.extend_from_slice(&u16b(700));
        t.extend_from_slice(&u16b(1));
        t.extend_from_slice(&u32b(u32::try_from(x.len()).unwrap()));
        t.extend_from_slice(&u32b(26));
        t.extend_from_slice(&u32b(0));
        t.extend_from_slice(x);
        t
    }

    #[test]
    fn locators() {
        assert_eq!(xmp_packet(&jpeg()).as_deref(), Some(ELEM));
        assert_eq!(xmp_packet(&png()).as_deref(), Some(ELEM));
        assert_eq!(xmp_packet(&tiff(true)).as_deref(), Some(ELEM));
        assert_eq!(xmp_packet(&tiff(false)).as_deref(), Some(ELEM));
        assert_eq!(read_xmp(&png()).unwrap().rating, Some(5));
        assert_eq!(xmp_packet(&[0xFF, 0xD8, 0xFF, 0xD9]), None);
        assert_eq!(xmp_packet(b"plain"), None);
        let mut c = png();
        // chunk data starts at sig+8; keyword is 17 bytes + NUL, then the flag.
        if let Some(b) = c.get_mut(PNG_SIG.len() + 8 + 18) {
            *b = 1; // compressed flag
        }
        assert_eq!(xmp_packet(&c), None);
    }

    #[test]
    fn truncation_never_panics() {
        for f in [jpeg(), png(), tiff(true), tiff(false)] {
            for n in 0..=f.len() {
                let _ = xmp_packet(&f[..n]);
                let _ = read_xmp(&f[..n]);
            }
        }
    }
}
