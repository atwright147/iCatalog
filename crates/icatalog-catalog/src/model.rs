#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
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
