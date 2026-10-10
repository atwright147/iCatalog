use rusqlite::{params, OptionalExtension};

use crate::{AssetMeta, Catalog, MetadataRaw, Result, Thumbnail};

impl Catalog {
    pub fn insert_thumbnail(&self, t: &Thumbnail) -> Result<()> {
        self.conn.execute(
            "INSERT INTO thumbnail (asset_id, size_class, source, format, width, height, orientation_applied, bytes, derived_from, generator_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                t.asset_id,
                t.size_class,
                t.source,
                t.format,
                t.width,
                t.height,
                i64::from(t.orientation_applied),
                t.bytes,
                t.derived_from,
                t.generator_version
            ],
        )?;
        Ok(())
    }

    pub fn thumbnail(&self, asset_id: i64, size_class: i64) -> Result<Option<Thumbnail>> {
        Ok(self
            .conn
            .query_row(
                "SELECT asset_id, size_class, source, format, width, height, orientation_applied, bytes, derived_from, generator_version
                 FROM thumbnail WHERE asset_id = ?1 AND size_class = ?2",
                params![asset_id, size_class],
                |r| {
                    let oa: i64 = r.get(6)?;
                    Ok(Thumbnail {
                        asset_id: r.get(0)?,
                        size_class: r.get(1)?,
                        source: r.get(2)?,
                        format: r.get(3)?,
                        width: r.get(4)?,
                        height: r.get(5)?,
                        orientation_applied: oa != 0,
                        bytes: r.get(7)?,
                        derived_from: r.get(8)?,
                        generator_version: r.get(9)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn insert_metadata_raw(&self, m: &MetadataRaw) -> Result<()> {
        self.conn.execute(
            "INSERT INTO metadata_raw (asset_id, scheme, key, seq, value) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![m.asset_id, m.scheme, m.key, m.seq, m.value],
        )?;
        Ok(())
    }

    pub fn metadata_raw_for_asset(&self, asset_id: i64) -> Result<Vec<MetadataRaw>> {
        let mut stmt = self.conn.prepare(
            "SELECT asset_id, scheme, key, seq, value FROM metadata_raw
             WHERE asset_id = ?1 ORDER BY scheme, key, seq",
        )?;
        let rows = stmt.query_map(params![asset_id], |r| {
            Ok(MetadataRaw {
                asset_id: r.get(0)?,
                scheme: r.get(1)?,
                key: r.get(2)?,
                seq: r.get(3)?,
                value: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn upsert_asset_meta(&self, m: &AssetMeta) -> Result<()> {
        self.conn.execute(
            "INSERT INTO asset_meta (asset_id, captured_local, tz_offset_min, make, model, lens, focal_length_mm, f_number, exposure_s, iso, gps_lat, gps_lon, orientation, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(asset_id) DO UPDATE SET
               captured_local = excluded.captured_local,
               tz_offset_min = excluded.tz_offset_min,
               make = excluded.make,
               model = excluded.model,
               lens = excluded.lens,
               focal_length_mm = excluded.focal_length_mm,
               f_number = excluded.f_number,
               exposure_s = excluded.exposure_s,
               iso = excluded.iso,
               gps_lat = excluded.gps_lat,
               gps_lon = excluded.gps_lon,
               orientation = excluded.orientation,
               duration_ms = excluded.duration_ms",
            params![
                m.asset_id,
                m.captured_local,
                m.tz_offset_min,
                m.make,
                m.model,
                m.lens,
                m.focal_length_mm,
                m.f_number,
                m.exposure_s,
                m.iso,
                m.gps_lat,
                m.gps_lon,
                m.orientation,
                m.duration_ms
            ],
        )?;
        Ok(())
    }

    pub fn asset_meta(&self, asset_id: i64) -> Result<Option<AssetMeta>> {
        Ok(self
            .conn
            .query_row(
                "SELECT asset_id, captured_local, tz_offset_min, make, model, lens, focal_length_mm, f_number, exposure_s, iso, gps_lat, gps_lon, orientation, duration_ms
                 FROM asset_meta WHERE asset_id = ?1",
                params![asset_id],
                |r| {
                    Ok(AssetMeta {
                        asset_id: r.get(0)?,
                        captured_local: r.get(1)?,
                        tz_offset_min: r.get(2)?,
                        make: r.get(3)?,
                        model: r.get(4)?,
                        lens: r.get(5)?,
                        focal_length_mm: r.get(6)?,
                        f_number: r.get(7)?,
                        exposure_s: r.get(8)?,
                        iso: r.get(9)?,
                        gps_lat: r.get(10)?,
                        gps_lon: r.get(11)?,
                        orientation: r.get(12)?,
                        duration_ms: r.get(13)?,
                    })
                },
            )
            .optional()?)
    }
}
