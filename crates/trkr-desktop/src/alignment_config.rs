//! Device-free Alignment panel configuration shared by the GUI, JSON and API.
use crate::{
    engine::{AlignmentUnit, MotionKind},
    inventory::Inventory,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub id: String,
    pub device_id: Option<String>,
    pub unit: AlignmentUnit,
    pub step: f64,
}
impl Panel {
    pub fn empty() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            device_id: None,
            unit: AlignmentUnit::Physical,
            step: 1.,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentLayout {
    pub delays: Vec<Panel>,
    pub scanners: Vec<Panel>,
}
impl AlignmentLayout {
    pub fn limit(kind: MotionKind) -> usize {
        match kind {
            MotionKind::Delay => 2,
            MotionKind::Scanner => 4,
        }
    }
    pub fn panels(&self, kind: MotionKind) -> &[Panel] {
        match kind {
            MotionKind::Delay => &self.delays,
            MotionKind::Scanner => &self.scanners,
        }
    }
    pub fn panels_mut(&mut self, kind: MotionKind) -> &mut Vec<Panel> {
        match kind {
            MotionKind::Delay => &mut self.delays,
            MotionKind::Scanner => &mut self.scanners,
        }
    }
    pub fn from_profile(raw: &Value, inventory: &Inventory) -> Result<Self, String> {
        let layout = if let Some(value) = raw.get("alignment") {
            serde_json::from_value(value.clone()).map_err(|error| format!("Alignment: {error}"))?
        } else {
            let defaults = |kind, ids: Vec<&String>| {
                ids.into_iter()
                    .take(Self::limit(kind))
                    .enumerate()
                    .map(|(index, device_id)| Panel {
                        id: format!(
                            "{}-panel-{}",
                            if kind == MotionKind::Delay {
                                "delay"
                            } else {
                                "scanner"
                            },
                            index + 1
                        ),
                        device_id: Some(device_id.clone()),
                        unit: AlignmentUnit::Physical,
                        step: 1.,
                    })
                    .collect()
            };
            Self {
                delays: defaults(
                    MotionKind::Delay,
                    inventory.delays.iter().map(|e| &e.id).collect(),
                ),
                scanners: defaults(
                    MotionKind::Scanner,
                    inventory.scanners.iter().map(|e| &e.id).collect(),
                ),
            }
        };
        layout.validate(inventory)?;
        Ok(layout)
    }
    pub fn validate(&self, inventory: &Inventory) -> Result<(), String> {
        let mut panel_ids = HashSet::new();
        for kind in [MotionKind::Delay, MotionKind::Scanner] {
            let panels = self.panels(kind);
            if panels.len() > Self::limit(kind) {
                return Err(format!(
                    "Alignment supports at most {} {:?} panels",
                    Self::limit(kind),
                    kind
                ));
            }
            let mut used = HashSet::new();
            for panel in panels {
                if panel.id.is_empty()
                    || panel.id.len() > 64
                    || !panel
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
                    || !panel_ids.insert(&panel.id)
                {
                    return Err("Alignment panel IDs must be unique, nonempty ASCII identifiers (max 64 bytes)".into());
                }
                if !panel.step.is_finite() || !(0.0..=1e12).contains(&panel.step) {
                    return Err("Alignment Step must be finite and between 0 and 1e12".into());
                }
                if let Some(id) = &panel.device_id {
                    let exists = match kind {
                        MotionKind::Delay => inventory.delays.iter().any(|e| &e.id == id),
                        MotionKind::Scanner => inventory.scanners.iter().any(|e| &e.id == id),
                    };
                    if !exists {
                        return Err(format!("Unknown Alignment {:?} device: {id}", kind));
                    }
                    if !used.insert(id) {
                        return Err(format!("Alignment device is already assigned: {id}"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn add(&mut self, kind: MotionKind) -> Result<(), String> {
        if self.panels(kind).len() >= Self::limit(kind) {
            return Err("Alignment panel limit reached".into());
        }
        self.panels_mut(kind).push(Panel::empty());
        Ok(())
    }
    pub fn remove(&mut self, kind: MotionKind, id: &str) {
        self.panels_mut(kind).retain(|panel| panel.id != id);
    }
    pub fn clear_device(&mut self, kind: MotionKind, device_id: &str) {
        for panel in self.panels_mut(kind) {
            if panel.device_id.as_deref() == Some(device_id) {
                panel.device_id = None;
            }
        }
    }
    pub fn value(&self) -> Value {
        serde_json::to_value(self).expect("validated finite Alignment settings")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    fn inventory() -> Inventory {
        Inventory::from_profile(&Settings::default().raw).unwrap()
    }

    #[test]
    fn older_profiles_seed_only_available_devices_and_empty_layout_is_preserved() {
        let inv = inventory();
        let layout = AlignmentLayout::from_profile(&serde_json::json!({}), &inv).unwrap();
        assert_eq!(layout.delays.len(), 1);
        assert_eq!(layout.scanners.len(), 1);
        let raw = serde_json::json!({"alignment":{"delays":[],"scanners":[]}});
        assert_eq!(
            AlignmentLayout::from_profile(&raw, &inv).unwrap(),
            AlignmentLayout::default()
        );
    }
    #[test]
    fn add_remove_keeps_surviving_ids_units_and_steps_then_roundtrips() {
        let inv = inventory();
        let mut layout = AlignmentLayout::from_profile(&serde_json::json!({}), &inv).unwrap();
        layout.add(MotionKind::Scanner).unwrap();
        layout.scanners[1].step = 2.5;
        layout.scanners[1].unit = AlignmentUnit::Corrected;
        let retained = layout.scanners[1].clone();
        let removed = layout.scanners[0].id.clone();
        layout.remove(MotionKind::Scanner, &removed);
        assert_eq!(layout.scanners, vec![retained]);
        let raw = serde_json::json!({"alignment":layout.value()});
        assert_eq!(AlignmentLayout::from_profile(&raw, &inv).unwrap(), layout);
        for _ in 1..4 {
            layout.add(MotionKind::Scanner).unwrap();
        }
        assert!(layout.add(MotionKind::Scanner).is_err());
        layout.validate(&inv).unwrap();
    }
    #[test]
    fn bad_steps_duplicate_devices_and_panel_ids_are_rejected() {
        let inv = inventory();
        let original = AlignmentLayout::from_profile(&serde_json::json!({}), &inv).unwrap();
        for step in [-1., f64::NAN, f64::INFINITY, 1e13] {
            let mut layout = original.clone();
            layout.scanners[0].step = step;
            assert!(layout.validate(&inv).is_err());
        }
        let mut layout = original.clone();
        layout.add(MotionKind::Scanner).unwrap();
        layout.scanners[1].device_id = layout.scanners[0].device_id.clone();
        assert!(layout.validate(&inv).is_err());
        layout.scanners[1].device_id = None;
        layout.scanners[1].id = layout.delays[0].id.clone();
        assert!(layout.validate(&inv).is_err());
        layout = original;
        layout.scanners[0].device_id = Some("missing".into());
        assert!(layout.validate(&inv).is_err());
    }
}
