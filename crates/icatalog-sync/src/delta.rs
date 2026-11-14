//! Bounded v2 binary messages. Lengths are checked before slicing/allocation.
use crate::{Result, SyncError};
use icatalog_catalog::{
    valid_catalogue_id, AnnotationState, CollectionState, SmartRules, MAX_SYNC_BYTES, MAX_SYNC_ROWS,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Annotation(AnnotationState),
    Collection(Box<CollectionState>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub base_revision: Option<i64>,
    pub value: Value,
}
impl Change {
    pub fn key(&self) -> String {
        match &self.value {
            Value::Annotation(a) => format!("asset:{}", a.uid),
            Value::Collection(c) => format!("collection:{}", c.uid),
        }
    }
    pub fn revision(&self) -> i64 {
        match &self.value {
            Value::Annotation(a) => a.revision,
            Value::Collection(c) => c.revision,
        }
    }
    pub fn same_value(&self, other: &Self) -> bool {
        match (&self.value, &other.value) {
            (Value::Annotation(a), Value::Annotation(b)) => {
                a.uid == b.uid
                    && a.rating == b.rating
                    && a.label == b.label
                    && a.keywords == b.keywords
            }
            (Value::Collection(a), Value::Collection(b)) => {
                a.uid == b.uid && a.name == b.name && a.rules == b.rules && a.members == b.members
            }
            _ => false,
        }
    }
}

pub fn current(c: &icatalog_catalog::Catalog) -> Result<Vec<Change>> {
    let mut out = Vec::new();
    for a in c.annotation_states()? {
        out.push(Change {
            base_revision: Some(a.revision),
            value: Value::Annotation(a),
        });
    }
    for a in c.collection_states()? {
        out.push(Change {
            base_revision: Some(a.revision),
            value: Value::Collection(Box::new(a)),
        });
    }
    Ok(out)
}

/// Compare the complete local user-edit state against the stored replica
/// baseline. Both `sync` and `sync-check` use this exact revision/value diff.
pub fn pending(
    catalogue_id: &str,
    baseline: Option<&(i64, Vec<u8>)>,
    catalog: &icatalog_catalog::Catalog,
) -> Result<(i64, Vec<Change>)> {
    let Some((generation, bytes)) = baseline else {
        return Ok((0, Vec::new()));
    };
    let (id, previous) = decode(bytes)?;
    if id != catalogue_id {
        return Err(SyncError::Integrity(
            "replica baseline belongs to another catalogue".into(),
        ));
    }
    let old: std::collections::BTreeMap<_, _> = previous
        .into_iter()
        .map(|record| (record.key(), record))
        .collect();
    let current = current(catalog)?;
    let current_keys: std::collections::BTreeSet<_> = current.iter().map(Change::key).collect();
    if old.keys().any(|key| !current_keys.contains(key)) {
        return Err(SyncError::Protocol("local server-managed data was deleted; restore it or recover from a backup before syncing".into()));
    }
    let changes = current
        .into_iter()
        .filter_map(|mut value| {
            let previous = old.get(&value.key());
            if previous.is_none_or(|p| !p.same_value(&value)) {
                value.base_revision = previous.map(Change::revision);
                Some(value)
            } else {
                None
            }
        })
        .collect();
    Ok((*generation, changes))
}

fn bad() -> SyncError {
    SyncError::Protocol("invalid or oversized v2 sync message".into())
}
fn number(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn count(out: &mut Vec<u8>, v: usize) -> Result<()> {
    out.extend_from_slice(&u32::try_from(v).map_err(|_| bad())?.to_le_bytes());
    Ok(())
}
fn text(out: &mut Vec<u8>, s: &str) -> Result<()> {
    if s.len() > 1024 * 1024 {
        return Err(bad());
    }
    count(out, s.len())?;
    out.extend_from_slice(s.as_bytes());
    if out.len() > MAX_SYNC_BYTES {
        return Err(bad());
    }
    Ok(())
}
fn texts(out: &mut Vec<u8>, v: &[String]) -> Result<()> {
    if v.len() > MAX_SYNC_ROWS {
        return Err(bad());
    }
    count(out, v.len())?;
    for s in v {
        text(out, s)?;
    }
    Ok(())
}
fn opt_text(out: &mut Vec<u8>, s: &Option<String>) -> Result<()> {
    out.push(u8::from(s.is_some()));
    if let Some(s) = s {
        text(out, s)?;
    }
    Ok(())
}

pub fn encode(id: &str, changes: &[Change]) -> Result<Vec<u8>> {
    if !valid_catalogue_id(id) {
        return Err(bad());
    }
    validate(changes)?;
    let mut out = b"ICAD2\0".to_vec();
    text(&mut out, id)?;
    count(&mut out, changes.len())?;
    for change in changes {
        out.push(match change.value {
            Value::Annotation(_) => 1,
            // Kind 3 extends collection rules with a maximum rating. Legacy
            // decoders reject unknown kinds rather than silently dropping it.
            Value::Collection(ref c)
                if c.rules.as_ref().is_some_and(|r| r.max_rating.is_some()) =>
            {
                3
            }
            Value::Collection(_) => 2,
        });
        number(&mut out, change.base_revision.unwrap_or(-1));
        number(&mut out, change.revision());
        match &change.value {
            Value::Annotation(a) => {
                text(&mut out, &a.uid)?;
                number(&mut out, a.rating);
                number(&mut out, a.label);
                texts(&mut out, &a.keywords)?;
            }
            Value::Collection(c) => {
                text(&mut out, &c.uid)?;
                text(&mut out, &c.name)?;
                out.push(u8::from(c.rules.is_some()));
                if let Some(r) = &c.rules {
                    for t in [
                        &r.format,
                        &r.captured_from,
                        &r.captured_to,
                        &r.camera,
                        &r.lens,
                        &r.keyword,
                        &r.text,
                    ] {
                        opt_text(&mut out, t)?;
                    }
                    number(&mut out, r.min_rating.unwrap_or(-1));
                    if r.max_rating.is_some() {
                        number(&mut out, r.max_rating.unwrap_or(-1));
                    }
                    number(&mut out, r.label.unwrap_or(-1));
                }
                texts(&mut out, &c.members)?;
            }
        }
    }
    if out.len() > MAX_SYNC_BYTES {
        return Err(bad());
    }
    Ok(out)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(bad)?;
        let b = self.b.get(self.at..end).ok_or_else(bad)?;
        self.at = end;
        Ok(b)
    }
    fn byte(&mut self) -> Result<u8> {
        self.take(1)?.first().copied().ok_or_else(bad)
    }
    fn number(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| bad())?,
        ))
    }
    fn count(&mut self, max: usize) -> Result<usize> {
        let n = u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| bad())?) as usize;
        if n > max {
            return Err(bad());
        }
        Ok(n)
    }
    fn text(&mut self) -> Result<String> {
        let n = self.count(1024 * 1024)?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad())
    }
    fn flag(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(bad()),
        }
    }
    fn opt_text(&mut self) -> Result<Option<String>> {
        if self.flag()? {
            Ok(Some(self.text()?))
        } else {
            Ok(None)
        }
    }
    fn texts(&mut self) -> Result<Vec<String>> {
        let n = self.count(MAX_SYNC_ROWS)?;
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(self.text()?);
        }
        Ok(out)
    }
}
fn opt_number(n: i64) -> Result<Option<i64>> {
    match n {
        -1 => Ok(None),
        0.. => Ok(Some(n)),
        _ => Err(bad()),
    }
}

pub fn decode(bytes: &[u8]) -> Result<(String, Vec<Change>)> {
    if bytes.len() > MAX_SYNC_BYTES {
        return Err(bad());
    }
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(6)? != b"ICAD2\0" {
        return Err(bad());
    }
    let id = r.text()?;
    if !valid_catalogue_id(&id) {
        return Err(bad());
    }
    let n = r.count(MAX_SYNC_ROWS * 2)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let kind = r.byte()?;
        let base_revision = opt_number(r.number()?)?;
        let revision = r.number()?;
        let uid = r.text()?;
        let value = match kind {
            1 => Value::Annotation(AnnotationState {
                uid,
                revision,
                rating: r.number()?,
                label: r.number()?,
                keywords: r.texts()?,
            }),
            2 | 3 => {
                let name = r.text()?;
                let rules = if r.flag()? {
                    Some(SmartRules {
                        format: r.opt_text()?,
                        captured_from: r.opt_text()?,
                        captured_to: r.opt_text()?,
                        camera: r.opt_text()?,
                        lens: r.opt_text()?,
                        keyword: r.opt_text()?,
                        text: r.opt_text()?,
                        min_rating: opt_number(r.number()?)?,
                        max_rating: if kind == 3 {
                            opt_number(r.number()?)?
                        } else {
                            None
                        },
                        label: opt_number(r.number()?)?,
                    })
                } else if kind == 2 {
                    None
                } else {
                    return Err(bad());
                };
                if kind == 3 && rules.as_ref().is_none_or(|r| r.max_rating.is_none()) {
                    return Err(bad());
                }
                Value::Collection(Box::new(CollectionState {
                    uid,
                    revision,
                    name,
                    rules,
                    members: r.texts()?,
                }))
            }
            _ => return Err(bad()),
        };
        out.push(Change {
            base_revision,
            value,
        });
    }
    if r.at != bytes.len() {
        return Err(bad());
    }
    validate(&out)?;
    Ok((id, out))
}

fn sorted_unique(v: &[String]) -> bool {
    v.windows(2).all(|p| matches!(p, [a,b] if a<b))
}
fn valid_uid(s: &str) -> bool {
    !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control)
}
pub fn validate(changes: &[Change]) -> Result<()> {
    if changes.len() > MAX_SYNC_ROWS * 2 {
        return Err(bad());
    }
    let mut keys = BTreeSet::new();
    for c in changes {
        if c.revision() < 0 || c.base_revision.is_some_and(|v| v < 0) || !keys.insert(c.key()) {
            return Err(bad());
        }
        match &c.value {
            Value::Annotation(a) => {
                if !valid_uid(&a.uid)
                    || !(0..=5).contains(&a.rating)
                    || !(0..=7).contains(&a.label)
                    || a.keywords.len() > MAX_SYNC_ROWS
                    || !sorted_unique(&a.keywords)
                    || a.keywords
                        .iter()
                        .any(|s| s.is_empty() || s.len() > 1024 * 1024)
                {
                    return Err(bad());
                }
            }
            Value::Collection(c) => {
                if !valid_uid(&c.uid)
                    || c.name.trim().is_empty()
                    || c.name.len() > 1024
                    || c.name.chars().any(char::is_control)
                    || c.members.len() > MAX_SYNC_ROWS
                    || !sorted_unique(&c.members)
                    || c.members.iter().any(|s| !valid_uid(s))
                {
                    return Err(bad());
                }
                if let Some(r) = &c.rules {
                    r.validate()?;
                    if !c.members.is_empty() {
                        return Err(bad());
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn encode_reply(generation: i64, conflicts: &[String]) -> Result<Vec<u8>> {
    if generation < 0 || conflicts.len() > MAX_SYNC_ROWS * 2 {
        return Err(bad());
    }
    let mut out = b"ICAR2\0".to_vec();
    number(&mut out, generation);
    count(&mut out, conflicts.len())?;
    for key in conflicts {
        text(&mut out, key)?;
    }
    Ok(out)
}
pub fn decode_reply(b: &[u8]) -> Result<(i64, Vec<String>)> {
    if b.len() > MAX_SYNC_BYTES {
        return Err(bad());
    }
    let mut r = Reader { b, at: 0 };
    if r.take(6)? != b"ICAR2\0" {
        return Err(bad());
    }
    let generation = r.number()?;
    let count = r.count(MAX_SYNC_ROWS * 2)?;
    let mut conflicts = Vec::new();
    for _ in 0..count {
        conflicts.push(r.text()?);
    }
    if generation < 0 || r.at != b.len() {
        return Err(bad());
    }
    Ok((generation, conflicts))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    #[test]
    fn maximum_rules_use_an_extended_kind_and_legacy_baselines_stay_readable() {
        let legacy = sample();
        let legacy_bytes = encode("catalogue", &legacy).unwrap();
        // Frozen kind-1/kind-2 v2 bytes: no maximum field existed in this baseline.
        let legacy_fixture = b"\x49\x43\x41\x44\x32\x00\x09\x00\x00\x00\x63\x61\x74\x61\x6c\x6f\
            \x67\x75\x65\x02\x00\x00\x00\x01\x02\x00\x00\x00\x00\x00\x00\x00\
            \x03\x00\x00\x00\x00\x00\x00\x00\x05\x00\x00\x00\x61\x73\x73\x65\
            \x74\x04\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\
            \x00\x01\x00\x00\x00\x07\x00\x00\x00\x68\x6f\x6c\x69\x64\x61\x79\
            \x02\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\
            \x00\x0e\x00\x00\x00\x63\x6f\x6c\x6c\x65\x63\x74\x69\x6f\x6e\x3a\
            \x6e\x65\x77\x05\x00\x00\x00\x53\x6d\x61\x72\x74\x01\x00\x00\x00\
            \x01\x06\x00\x00\x00\x43\x61\x6d\x65\x72\x61\x00\x00\x00\x04\x00\
            \x00\x00\x00\x00\x00\x00\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\
            \x00\x00";
        assert_eq!(legacy_bytes, legacy_fixture);
        assert_eq!(decode(legacy_fixture).unwrap().1, legacy);
        let mut changes = sample();
        if let Value::Collection(c) = &mut changes[1].value {
            c.rules.as_mut().unwrap().max_rating = Some(5);
        }
        let bytes = encode("catalogue", &changes).unwrap();
        assert_eq!(decode(&bytes).unwrap().1, changes);
        // The first annotation has unchanged bytes. A legacy decoder sees an
        // unknown collection kind and rejects it before reading its fields.
        let mut reader = Reader { b: &bytes, at: 0 };
        reader.take(6).unwrap();
        reader.text().unwrap();
        reader.count(MAX_SYNC_ROWS).unwrap();
        assert_eq!(reader.byte().unwrap(), 1);
        reader.number().unwrap();
        reader.number().unwrap();
        reader.text().unwrap();
        reader.number().unwrap();
        reader.number().unwrap();
        reader.texts().unwrap();
        assert_eq!(reader.byte().unwrap(), 3);
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err());
        }
        for index in 0..bytes.len() {
            let mut mutated = bytes.clone();
            mutated[index] ^= 255;
            assert!(std::panic::catch_unwind(|| decode(&mutated)).is_ok());
        }
        if let Value::Collection(c) = &mut changes[1].value {
            c.rules.as_mut().unwrap().max_rating = Some(3);
        }
        assert!(encode("catalogue", &changes).is_err());
    }
    #[test]
    fn reply_can_report_conflicts_from_both_entity_types_at_the_supported_bound() {
        let conflicts = vec!["asset:key".to_owned(); MAX_SYNC_ROWS + 1];
        let bytes = encode_reply(1, &conflicts).unwrap();
        assert_eq!(decode_reply(&bytes).unwrap(), (1, conflicts));
        assert!(encode("bad\nid", &[]).is_err());
    }
    fn sample() -> Vec<Change> {
        vec![
            Change {
                base_revision: Some(2),
                value: Value::Annotation(AnnotationState {
                    uid: "asset".into(),
                    revision: 3,
                    rating: 4,
                    label: 2,
                    keywords: vec!["holiday".into()],
                }),
            },
            Change {
                base_revision: None,
                value: Value::Collection(Box::new(CollectionState {
                    uid: "collection:new".into(),
                    revision: 0,
                    name: "Smart".into(),
                    rules: Some(SmartRules {
                        min_rating: Some(4),
                        camera: Some("Camera".into()),
                        ..Default::default()
                    }),
                    members: vec![],
                })),
            },
        ]
    }
    #[test]
    fn wire_round_trips_and_hostile_lengths_truncation_and_mutations_never_panic() {
        let values = sample();
        let bytes = encode("catalogue", &values).unwrap();
        assert_eq!(
            decode(&bytes).unwrap(),
            ("catalogue".into(), values.clone())
        );
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err());
        }
        for index in 0..bytes.len() {
            let mut mutated = bytes.clone();
            mutated[index] ^= 255;
            assert!(std::panic::catch_unwind(|| decode(&mutated)).is_ok());
        }
        assert!(decode(b"ICAD2\0\xff\xff\xff\xff").is_err());
        assert!(decode_reply(b"ICAR2\0").is_err());
        let mut duplicate = values.clone();
        duplicate.extend(values);
        assert!(encode("catalogue", &duplicate).is_err());
        let reply = encode_reply(7, &["asset:asset".into()]).unwrap();
        assert_eq!(
            decode_reply(&reply).unwrap(),
            (7, vec!["asset:asset".into()])
        );
    }
}
