use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    /// Import source this folder belongs to (v4, D21); `None` for legacy trees.
    pub source_id: Option<i64>,
    /// POSIX path relative to the source root; `Some("")` is the root folder.
    pub path: Option<String>,
    pub name: String,
}

/// Where a group of assets was imported from (v4, D21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: i64,
    /// `local`, `optical`, `removable` or `network`.
    pub kind: String,
    /// Human label, normally the import root's directory name.
    pub label: String,
    /// Stable identity; currently the canonical import-root path.
    pub identity: String,
    /// Last path at which the source was seen (used to detect an offline disc or share).
    pub last_mount_path: Option<String>,
    pub last_seen_utc: Option<String>,
    /// Volume/share root the source came from (e.g. `/`, `/Volumes/NAS`).
    pub device_id: Option<String>,
    /// Human device label (`This Mac`, `NAS`, `DVD_01`, ...).
    pub device_label: Option<String>,
}

impl Source {
    /// Device id and label, deriving them from the mount path when the row
    /// predates migration v5.
    pub fn device(&self) -> (String, String) {
        match (&self.device_id, &self.device_label) {
            (Some(id), Some(label)) => (id.clone(), label.clone()),
            _ => device_of(self.last_mount_path.as_deref().unwrap_or(&self.identity)),
        }
    }
}

/// Best-effort device identity for a path: the volume or share root.
///
/// Unix: `/Volumes/<name>`, `/mnt/<name>` and `/media/<name>` are mounts; any
/// other path belongs to the boot volume `/`. Windows: the drive prefix or UNC
/// share. This is heuristic (no filesystem call), so it never fails and never
/// touches a network mount.
pub fn device_of(path: &str) -> (String, String) {
    let root = device_root(Path::new(path));
    let label = if root == Path::new("/") {
        "This Mac".to_owned()
    } else {
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| root.to_string_lossy().into_owned())
    };
    (root.to_string_lossy().into_owned(), label)
}

#[cfg(windows)]
fn device_root(path: &Path) -> PathBuf {
    match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => PathBuf::from(prefix.as_os_str()),
        _ => PathBuf::from("\\"),
    }
}

#[cfg(not(windows))]
fn device_root(path: &Path) -> PathBuf {
    let comps: Vec<std::path::Component<'_>> = path.components().collect();
    if let Some(std::path::Component::RootDir) = comps.first() {
        if let (Some(std::path::Component::Normal(a)), Some(std::path::Component::Normal(b))) =
            (comps.get(1), comps.get(2))
        {
            let a = a.to_string_lossy();
            if matches!(a.as_ref(), "Volumes" | "mnt" | "media") {
                return Path::new("/").join(&*a).join(b);
            }
        }
    }
    PathBuf::from("/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub asset_id: i64,
    pub size_class: i64,
    pub source: String,
    pub format: String,
    pub width: i64,
    pub height: i64,
    pub orientation_applied: bool,
    pub bytes: Vec<u8>,
    pub derived_from: Option<String>,
    pub generator_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataRaw {
    pub asset_id: i64,
    pub scheme: String,
    pub key: String,
    pub seq: i64,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetMeta {
    pub asset_id: i64,
    pub captured_local: Option<String>,
    pub tz_offset_min: Option<i64>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub focal_length_mm: Option<f64>,
    pub f_number: Option<f64>,
    pub exposure_s: Option<f64>,
    pub iso: Option<i64>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub orientation: Option<i64>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyword {
    pub id: i64,
    pub name: String,
    pub name_raw: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub asset_id: i64,
    pub rating: i64,
    pub label: i64,
    pub caption: Option<String>,
    pub headline: Option<String>,
    pub creator: Option<String>,
    pub copyright: Option<String>,
    pub updated_utc: Option<String>,
}
