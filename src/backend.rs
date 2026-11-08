//! Vendor backends: the one place that knows how to talk to a laptop's embedded controller.
//!
//! The UI and the worker only see [`VendorBackend`]. [`detect`] maps the BIOS manufacturer to a
//! [`Vendor`] and returns the matching implementation. Until a native backend exists for a brand,
//! [`Simulated`] serves it with physically plausible telemetry driven by the real CPU load, so the
//! whole app works end to end. A native backend implements the same trait and is returned from
//! [`detect`] instead; [`Vendor::native_interface`] names the firmware interface each brand exposes.

use std::time::Instant;

use crate::curve::{CurveSet, Fan, FanCurve, Mode};
use crate::system::{CpuLoad, DeviceInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Asus,
    Lenovo,
    Hp,
    Msi,
    Dell,
    Acer,
    Gigabyte,
    Framework,
    Unknown,
}

type VendorRow = (&'static [&'static str], Vendor, &'static str, &'static str);

/// `(manufacturer prefixes, vendor, product line, native firmware interface)`.
const VENDORS: [VendorRow; 8] = [
    (&["asus"], Vendor::Asus, "ASUS ROG / TUF", r"ATKACPI (\.\ATKACPI) DEVS/DSTS"),
    (&["lenovo"], Vendor::Lenovo, "Lenovo Legion / LOQ", "WMI LENOVO_GAMEZONE_DATA"),
    (&["hp", "hewlett"], Vendor::Hp, "HP Omen / Victus", "WMI hpqBIntM thermal policy"),
    (&["micro-star", "msi"], Vendor::Msi, "MSI", "WMI MSI_ACPI EC registers"),
    (&["dell", "alienware"], Vendor::Dell, "Dell / Alienware", "WMI AWCCWmiMethodFunction"),
    (&["acer"], Vendor::Acer, "Acer Predator / Nitro", "WMI AcerGamingFunction"),
    (&["gigabyte", "aorus"], Vendor::Gigabyte, "Gigabyte / AORUS", "WMI GB_WMIACPI"),
    (&["framework"], Vendor::Framework, "Framework", "Chrome EC host commands"),
];

impl Vendor {
    /// Match the `SystemManufacturer` BIOS string (case-insensitive prefix).
    pub fn from_manufacturer(raw: &str) -> Vendor {
        let m = raw.to_lowercase();
        VENDORS.iter().find(|v| v.0.iter().any(|p| m.starts_with(p))).map_or(Vendor::Unknown, |v| v.1)
    }

    fn row(self) -> Option<&'static VendorRow> {
        VENDORS.iter().find(|v| v.1 == self)
    }

    pub fn label(self) -> &'static str {
        self.row().map_or("Generic Windows laptop", |v| v.2)
    }

    /// The firmware interface a native backend for this vendor talks to.
    pub fn native_interface(self) -> &'static str {
        self.row().map_or("Windows power plans only", |v| v.3)
    }
}

/// One telemetry sample.
#[derive(Clone, Debug, Default)]
pub struct Sensors {
    pub cpu_temp: f32,
    pub gpu_temp: f32,
    pub vrm_temp: f32,
    pub ssd_temp: f32,
    pub cpu_fan_rpm: f32,
    pub gpu_fan_rpm: f32,
    pub cpu_power_w: f32,
    pub gpu_power_w: f32,
    pub cpu_load: f32,
}

/// Everything Fanatic needs from a laptop. Implementations run on the worker thread only.
pub trait VendorBackend: Send {
    /// Short name shown in the sidebar, e.g. "Lenovo Legion / LOQ (simulated)".
    fn name(&self) -> String;
    /// Switch the firmware thermal / power mode.
    fn set_mode(&mut self, mode: Mode) -> Result<(), String>;
    /// Upload the fan curve of one fan for the current mode.
    fn fan_curve(&mut self, fan: Fan, curve: &FanCurve) -> Result<(), String>;
    /// Stop charging at `percent` (typically 60, 80 or 100).
    fn set_charge_limit(&mut self, percent: u8) -> Result<(), String>;
    /// Read temperatures, fan speeds and power draw.
    fn read_sensors(&mut self) -> Sensors;
}

/// Pick the backend for this machine.
pub fn detect(device: &DeviceInfo) -> Box<dyn VendorBackend> {
    let vendor = Vendor::from_manufacturer(&device.manufacturer);
    // Native backends slot in here, e.g. `Vendor::Asus => Box::new(AsusAcpi::open()?)`,
    // falling back to the simulator when the firmware interface cannot be opened.
    Box::new(Simulated::new(vendor))
}

/// A laptop in software: first-order thermal model, fan inertia, curve-following fans.
pub struct Simulated {
    vendor: Vendor,
    mode: Mode,
    curves: CurveSet,
    charge_limit: u8,
    cpu: CpuLoad,
    load: f32,
    temps: [f32; 4],
    rpm: [f32; 2],
    started: Instant,
    last: Instant,
}

impl Simulated {
    const MAX_RPM: f32 = 5800.0;
    const AMBIENT_C: f32 = 27.0;

    pub fn new(vendor: Vendor) -> Self {
        let now = Instant::now();
        Self {
            vendor,
            mode: Mode::Balanced,
            curves: CurveSet::preset(Mode::Balanced),
            charge_limit: 100,
            cpu: CpuLoad::default(),
            load: 0.2,
            temps: [52.0, 48.0, 45.0, 38.0],
            rpm: [2400.0, 2500.0],
            started: now,
            last: now,
        }
    }
}

impl VendorBackend for Simulated {
    fn name(&self) -> String {
        format!("{} (simulated, stops at {}%)", self.vendor.label(), self.charge_limit)
    }

    fn set_mode(&mut self, mode: Mode) -> Result<(), String> {
        self.mode = mode;
        Ok(())
    }

    fn fan_curve(&mut self, fan: Fan, curve: &FanCurve) -> Result<(), String> {
        if curve.points.len() != 8 {
            return Err(format!("{fan:?} fan curve needs exactly 8 points"));
        }
        *self.curves.get_mut(fan) = curve.clone();
        Ok(())
    }

    fn set_charge_limit(&mut self, percent: u8) -> Result<(), String> {
        if !(20..=100).contains(&percent) {
            return Err(format!("charge limit {percent}% out of range"));
        }
        self.charge_limit = percent;
        Ok(())
    }

    fn read_sensors(&mut self) -> Sensors {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().clamp(0.05, 5.0);
        let t = now.duration_since(self.started).as_secs_f32();
        self.last = now;
        let real = self.cpu.sample().unwrap_or(self.load);
        self.load += (real - self.load) * (dt / 2.0).min(1.0);

        let heat = self.mode.heat_factor();
        let gpu_load = (0.45 + 0.35 * (t / 23.0).sin() + 0.1 * (t / 3.7).sin()).clamp(0.05, 1.0);
        let (cpu_w, gpu_w) = ((8.0 + 95.0 * self.load) * heat, (12.0 + 120.0 * gpu_load) * heat);
        // Airflow (0..1 per fan) pulls the equilibrium temperature down.
        let air = self.rpm.map(|r| r / Self::MAX_RPM);
        let targets = [
            Self::AMBIENT_C + cpu_w * (0.62 - 0.28 * air[0]),
            Self::AMBIENT_C + gpu_w * (0.5 - 0.22 * air[1]),
            Self::AMBIENT_C + (cpu_w + gpu_w) * 0.18,
            Self::AMBIENT_C + 10.0 + 6.0 * self.load,
        ];
        for (temp, target) in self.temps.iter_mut().zip(targets) {
            *temp += (target.min(99.0) - *temp) * (dt / 6.0).min(1.0);
        }
        for (i, fan) in [Fan::Cpu, Fan::Gpu].into_iter().enumerate() {
            let goal = self.curves.get(fan).duty_at(self.temps[i]) / 100.0 * Self::MAX_RPM;
            self.rpm[i] += (goal - self.rpm[i]) * (dt / 1.5).min(1.0);
        }
        let [cpu_temp, gpu_temp, vrm_temp, ssd_temp] = self.temps;
        Sensors {
            cpu_temp,
            gpu_temp,
            vrm_temp,
            ssd_temp,
            cpu_fan_rpm: self.rpm[0],
            gpu_fan_rpm: self.rpm[1],
            cpu_power_w: cpu_w,
            gpu_power_w: gpu_w,
            cpu_load: self.load,
        }
    }
}
