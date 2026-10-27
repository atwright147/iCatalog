//! Asset selection and rectangle geometry are local view state.
use eframe::egui;
use icatalog_catalog::Asset;
use std::collections::HashSet;

#[derive(Clone, Default)]
pub struct AssetSelection {
    pub ids: HashSet<i64>,
    pub primary: Option<i64>,
    anchor: Option<i64>,
}

impl AssetSelection {
    pub fn click(&mut self, id: i64, assets: &[Asset], modifiers: egui::Modifiers) {
        let toggle = modifiers.ctrl || modifiers.command;
        if modifiers.shift {
            let start = self
                .anchor
                .and_then(|anchor| assets.iter().position(|a| a.id == anchor));
            let end = assets.iter().position(|a| a.id == id);
            if !toggle {
                self.ids.clear();
            }
            match (start, end) {
                (Some(a), Some(b)) => {
                    if let Some(range) = assets.get(a.min(b)..=a.max(b)) {
                        self.ids.extend(range.iter().map(|a| a.id));
                    }
                }
                _ => {
                    self.ids.insert(id);
                    self.anchor = Some(id);
                }
            }
        } else if toggle {
            if !self.ids.remove(&id) {
                self.ids.insert(id);
            }
            self.anchor = Some(id);
        } else {
            self.ids.clear();
            self.ids.insert(id);
            self.anchor = Some(id);
        }
        self.primary = self.ids.contains(&id).then_some(id);
        self.retain(assets);
    }

    pub fn focus(&mut self, id: i64) {
        if !self.ids.contains(&id) {
            self.ids.clear();
            self.ids.insert(id);
            self.anchor = Some(id);
        }
        self.primary = Some(id);
    }

    pub fn ordered(&self, assets: &[Asset]) -> Vec<i64> {
        assets
            .iter()
            .filter(|a| self.ids.contains(&a.id))
            .map(|a| a.id)
            .collect()
    }

    pub fn retain(&mut self, assets: &[Asset]) {
        let visible: HashSet<i64> = assets.iter().map(|a| a.id).collect();
        self.ids.retain(|id| visible.contains(id));
        if !self.primary.is_some_and(|id| self.ids.contains(&id)) {
            self.primary = assets
                .iter()
                .find(|a| self.ids.contains(&a.id))
                .map(|a| a.id);
        }
        if self.ids.is_empty() || !self.anchor.is_some_and(|id| visible.contains(&id)) {
            self.anchor = self.primary;
        }
    }

    pub fn rectangle(
        &mut self,
        base: &Self,
        hits: HashSet<i64>,
        modifiers: egui::Modifiers,
        assets: &[Asset],
    ) {
        let toggle = modifiers.ctrl || modifiers.command;
        self.ids = if toggle {
            base.ids.symmetric_difference(&hits).copied().collect()
        } else if modifiers.shift {
            base.ids.union(&hits).copied().collect()
        } else {
            hits
        };
        self.primary = base.primary.filter(|id| self.ids.contains(id)).or_else(|| {
            assets
                .iter()
                .find(|a| self.ids.contains(&a.id))
                .map(|a| a.id)
        });
        self.anchor = self.primary;
    }
}

pub struct Marquee {
    pub start: egui::Pos2,
    pub base: AssetSelection,
    pub modifiers: egui::Modifiers,
}

/// Geometry of all cells, including rows that have not been rendered.
pub struct AssetLayout {
    pub origin: egui::Pos2,
    pub columns: usize,
    pub cell: egui::Vec2,
    pub spacing: egui::Vec2,
}

impl AssetLayout {
    pub fn hits(&self, rect: egui::Rect, assets: &[Asset]) -> HashSet<i64> {
        let mut hits = HashSet::new();
        if !rect.is_finite()
            || !self.origin.is_finite()
            || !self.cell.is_finite()
            || !self.spacing.is_finite()
            || self.cell.x < 0.0
            || self.cell.y < 0.0
        {
            return hits;
        }
        let columns = self.columns.max(1);
        let stride = (self.cell.y + self.spacing.y).max(1.0);
        let first = ((rect.top() - self.origin.y) / stride).floor().max(0.0) as usize;
        let last = (((rect.bottom() - self.origin.y) / stride).floor().max(0.0) as usize)
            .saturating_add(1)
            .min(assets.len().div_ceil(columns));
        for row in first..last {
            for col in 0..columns {
                let index = row.saturating_mul(columns).saturating_add(col);
                let Some(asset) = assets.get(index) else {
                    break;
                };
                let origin = self.origin
                    + egui::vec2(
                        col as f32 * (self.cell.x + self.spacing.x),
                        row as f32 * stride,
                    );
                if rect.intersects(egui::Rect::from_min_size(origin, self.cell)) {
                    hits.insert(asset.id);
                }
            }
        }
        hits
    }
}
