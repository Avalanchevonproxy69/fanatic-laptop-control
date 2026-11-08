//! Battery health history and the "you will hit 80% capacity in ~N months" forecast.
//!
//! Windows reports the current full-charge capacity (`SYSTEM_BATTERY_STATE::MaxCapacity`).
//! Fanatic stores one sample per day and fits a least-squares line through the history.
//! The baseline is the design capacity when known (set it in settings, or plug in an
//! `IOCTL_BATTERY_QUERY_INFORMATION` / `BatteryInformation` reader), otherwise the best
//! capacity ever observed.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Keep two years of daily samples.
const MAX_SAMPLES: usize = 730;
const DAYS_PER_MONTH: f32 = 30.44;
/// Capacity threshold most vendors consider "worn" (and most warranties use).
pub const WORN_FRACTION: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapacitySample {
    /// Days since 1970-01-01.
    pub day: u32,
    pub full_mwh: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forecast {
    /// Current capacity as a fraction of the baseline.
    pub health: f32,
    /// Capacity lost per month, as a fraction of the baseline (positive = wearing).
    pub loss_per_month: f32,
    /// Months until `WORN_FRACTION`; `None` when capacity is flat or rising.
    pub months_to_worn: Option<f32>,
}

pub fn today() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| (d.as_secs() / 86_400) as u32)
}

/// Add today's sample (replacing an earlier one from the same day). Returns true when changed.
pub fn record(history: &mut Vec<CapacitySample>, day: u32, full_mwh: u32) -> bool {
    match history.last_mut() {
        Some(last) if last.day == day => {
            let changed = last.full_mwh != full_mwh;
            last.full_mwh = full_mwh;
            changed
        }
        _ => {
            history.push(CapacitySample { day, full_mwh });
            if history.len() > MAX_SAMPLES {
                history.drain(..history.len() - MAX_SAMPLES);
            }
            true
        }
    }
}

/// Fit capacity over time. Needs at least two samples spanning a week.
pub fn forecast(history: &[CapacitySample], design_mwh: Option<u32>) -> Option<Forecast> {
    let last = history.last()?;
    let baseline = design_mwh.or_else(|| history.iter().map(|s| s.full_mwh).max())? as f32;
    if baseline <= 0.0 {
        return None;
    }
    let health = last.full_mwh as f32 / baseline;
    let span = last.day.saturating_sub(history.first()?.day);
    if history.len() < 2 || span < 7 {
        return Some(Forecast { health, loss_per_month: 0.0, months_to_worn: None });
    }
    let n = history.len() as f32;
    let (sx, sy) = history.iter().fold((0.0, 0.0), |(a, b), s| (a + s.day as f32, b + s.full_mwh as f32));
    let (mx, my) = (sx / n, sy / n);
    let (mut num, mut den) = (0.0, 0.0);
    for s in history {
        let dx = s.day as f32 - mx;
        num += dx * (s.full_mwh as f32 - my);
        den += dx * dx;
    }
    let slope_per_day = if den > 0.0 { num / den } else { 0.0 };
    let loss_per_month = -slope_per_day * DAYS_PER_MONTH / baseline;
    let months_to_worn = (loss_per_month > 1e-4 && health > WORN_FRACTION).then(|| (health - WORN_FRACTION) / loss_per_month);
    Some(Forecast { health, loss_per_month, months_to_worn })
}
