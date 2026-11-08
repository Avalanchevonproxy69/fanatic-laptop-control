//! Performance modes and piecewise-linear fan curves.

use serde::{Deserialize, Serialize};

use crate::system::PowerScheme;

/// Coolest and hottest points a curve may use, in degrees Celsius.
pub const TEMP_MIN: f32 = 30.0;
pub const TEMP_MAX: f32 = 100.0;
/// Minimum spacing between two curve points that EC firmwares accept.
const MIN_GAP_C: f32 = 3.0;

/// A performance mode. Vendors name these differently (Quiet / Whisper, Balanced,
/// Performance, Turbo / Overboost); Fanatic shows one vocabulary for every brand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Mode {
    Silent,
    Balanced,
    Performance,
    Turbo,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Silent, Mode::Balanced, Mode::Performance, Mode::Turbo];

    pub fn label(self) -> &'static str {
        ["Silent", "Balanced", "Performance", "Turbo"][self as usize]
    }

    /// Next mode in the hotkey cycle (Silent, Balanced, Performance, Turbo, Silent ...).
    pub fn next(self) -> Mode {
        Mode::ALL[(self as usize + 1) % Mode::ALL.len()]
    }

    /// The Windows power plan applied together with the vendor mode.
    pub fn power_scheme(self) -> PowerScheme {
        match self {
            Mode::Silent => PowerScheme::PowerSaver,
            Mode::Balanced => PowerScheme::Balanced,
            Mode::Performance | Mode::Turbo => PowerScheme::HighPerformance,
        }
    }

    /// Relative package power used by the simulated backend.
    pub fn heat_factor(self) -> f32 {
        [0.55, 0.8, 1.0, 1.18][self as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fan {
    Cpu,
    Gpu,
}

/// Eight `(temperature C, duty %)` points, the table format most EC firmwares use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FanCurve {
    pub points: Vec<[f32; 2]>,
}

impl FanCurve {
    /// Factory-style curve for a mode.
    pub fn preset(mode: Mode) -> Self {
        let duty: [f32; 8] = match mode {
            Mode::Silent => [0.0, 0.0, 18.0, 26.0, 34.0, 45.0, 62.0, 80.0],
            Mode::Balanced => [0.0, 15.0, 25.0, 35.0, 46.0, 60.0, 78.0, 95.0],
            Mode::Performance => [15.0, 25.0, 35.0, 48.0, 60.0, 74.0, 90.0, 100.0],
            Mode::Turbo => [25.0, 35.0, 48.0, 62.0, 76.0, 88.0, 100.0, 100.0],
        };
        let temps = [40.0, 50.0, 60.0, 67.0, 74.0, 81.0, 88.0, 95.0];
        Self { points: temps.iter().zip(duty).map(|(t, d)| [*t, d]).collect() }
    }

    /// Duty cycle (0..=100 %) at `temp`, linearly interpolated and clamped at both ends.
    pub fn duty_at(&self, temp: f32) -> f32 {
        let Some(first) = self.points.first() else { return 0.0 };
        if temp <= first[0] {
            return first[1];
        }
        for w in self.points.windows(2) {
            let ([t0, d0], [t1, d1]) = (w[0], w[1]);
            if temp <= t1 {
                return d0 + (d1 - d0) * if t1 > t0 { (temp - t0) / (t1 - t0) } else { 1.0 };
            }
        }
        self.points.last().map_or(0.0, |p| p[1])
    }

    /// Move point `i`, keeping temperatures strictly increasing and duty non-decreasing
    /// (firmware rejects anything else).
    pub fn set_point(&mut self, i: usize, temp: f32, duty: f32) {
        let n = self.points.len();
        if i >= n {
            return;
        }
        let (prev, next) = (i.checked_sub(1).map(|k| self.points[k]), self.points.get(i + 1).copied());
        let lo_t = prev.map_or(TEMP_MIN, |p| p[0] + MIN_GAP_C);
        let hi_t = next.map_or(TEMP_MAX, |p| p[0] - MIN_GAP_C).max(lo_t);
        let (lo_d, hi_d) = (prev.map_or(0.0, |p| p[1]), next.map_or(100.0, |p| p[1]));
        self.points[i] = [temp.clamp(lo_t, hi_t).round(), duty.clamp(lo_d, hi_d).round()];
    }
}

/// The CPU and GPU curve of one mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurveSet {
    /// Indexed by `Fan as usize`: CPU first, then GPU.
    pub fans: [FanCurve; 2],
}

impl CurveSet {
    pub fn preset(mode: Mode) -> Self {
        Self { fans: [FanCurve::preset(mode), FanCurve::preset(mode)] }
    }

    pub fn get(&self, fan: Fan) -> &FanCurve {
        &self.fans[fan as usize]
    }

    pub fn get_mut(&mut self, fan: Fan) -> &mut FanCurve {
        &mut self.fans[fan as usize]
    }
}
