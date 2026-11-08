//! Per-app profiles and the "Silent hours" schedule.
//!
//! Resolution order: a running app with a rule wins (the most demanding mode if several),
//! then Silent hours, then the mode the user picked by hand.

use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::curve::Mode;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppRule {
    /// Executable name, matched case-insensitively (e.g. `eldenring.exe`).
    pub exe: String,
    /// Friendly name for the list.
    pub label: String,
    pub mode: Mode,
}

impl AppRule {
    fn new(exe: &str, label: &str, mode: Mode) -> Self {
        Self { exe: exe.to_owned(), label: label.to_owned(), mode }
    }
}

pub fn default_rules() -> Vec<AppRule> {
    vec![
        AppRule::new("cyberpunk2077.exe", "Cyberpunk 2077", Mode::Performance),
        AppRule::new("eldenring.exe", "Elden Ring", Mode::Performance),
        AppRule::new("cs2.exe", "Counter-Strike 2", Mode::Turbo),
        AppRule::new("blender.exe", "Blender", Mode::Performance),
        AppRule::new("ms-teams.exe", "Microsoft Teams", Mode::Silent),
    ]
}

/// A nightly window (may wrap midnight) during which fans stay in Silent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SilentHours {
    pub enabled: bool,
    /// Minutes after local midnight.
    pub start_min: u16,
    pub end_min: u16,
}

impl Default for SilentHours {
    fn default() -> Self {
        Self { enabled: true, start_min: 23 * 60, end_min: 7 * 60 + 30 }
    }
}

impl SilentHours {
    pub fn contains(&self, minute: u16) -> bool {
        if !self.enabled || self.start_min == self.end_min {
            return false;
        }
        if self.start_min < self.end_min {
            (self.start_min..self.end_min).contains(&minute)
        } else {
            minute >= self.start_min || minute < self.end_min
        }
    }
}

pub fn local_minute_of_day() -> u16 {
    // SAFETY: GetLocalTime has no failure mode.
    let t = unsafe { GetLocalTime() };
    t.wHour * 60 + t.wMinute
}

/// Why a mode was chosen; shown in the UI and in the mode-switch overlay.
#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    App(String),
    SilentHours,
    Manual,
}

impl Reason {
    pub fn describe(&self) -> String {
        match self {
            Reason::App(label) => format!("{label} is running"),
            Reason::SilentHours => "Silent hours".to_owned(),
            Reason::Manual => "Manual".to_owned(),
        }
    }
}

pub fn resolve(rules: &[AppRule], running: &[String], silent: &SilentHours, minute: u16, manual: Mode) -> (Mode, Reason) {
    let hit = rules.iter().filter(|r| running.iter().any(|p| p.eq_ignore_ascii_case(&r.exe))).max_by_key(|r| r.mode);
    match hit {
        Some(rule) => (rule.mode, Reason::App(rule.label.clone())),
        None if silent.contains(minute) => (Mode::Silent, Reason::SilentHours),
        None => (manual, Reason::Manual),
    }
}
