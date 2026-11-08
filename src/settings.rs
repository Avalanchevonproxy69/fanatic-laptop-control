//! Settings persisted as JSON in `%APPDATA%\Fanatic\settings.json`.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::battery::CapacitySample;
use crate::curve::{CurveSet, Mode};
use crate::profiles::{AppRule, SilentHours, default_rules};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Mode picked by hand; used when no app rule or schedule applies.
    pub mode: Mode,
    pub charge_limit: u8,
    pub curves: BTreeMap<Mode, CurveSet>,
    /// Per-app profiles and Silent hours on / off.
    pub automation: bool,
    pub rules: Vec<AppRule>,
    pub silent_hours: SilentHours,
    /// Show the mode-switch overlay when the mode changes.
    pub show_overlay: bool,
    pub design_capacity_mwh: Option<u32>,
    pub capacity_history: Vec<CapacitySample>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Balanced,
            charge_limit: 80,
            curves: Mode::ALL.iter().map(|m| (*m, CurveSet::preset(*m))).collect(),
            automation: true,
            rules: default_rules(),
            silent_hours: SilentHours::default(),
            show_overlay: true,
            design_capacity_mwh: None,
            capacity_history: Vec::new(),
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Fanatic").join("settings.json"))
    }

    /// Load settings, falling back to defaults on a missing or unreadable file.
    pub fn load() -> Self {
        let mut s: Settings = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        for m in Mode::ALL {
            s.curves.entry(m).or_insert_with(|| CurveSet::preset(m));
        }
        s
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("APPDATA is not set")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn curves_for(&self, mode: Mode) -> CurveSet {
        self.curves.get(&mode).cloned().unwrap_or_else(|| CurveSet::preset(mode))
    }
}
