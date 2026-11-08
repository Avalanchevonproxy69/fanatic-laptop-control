//! Thin, safe wrappers over the Win32 APIs Fanatic needs on every laptop, regardless of brand:
//! battery state, the active power plan, BIOS identity strings, running processes, CPU load.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, FILETIME, HLOCAL, LocalFree};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Power::{
    CallNtPowerInformation, GetSystemPowerStatus, PowerGetActiveScheme, PowerSetActiveScheme, SYSTEM_BATTERY_STATE,
    SYSTEM_POWER_STATUS, SystemBatteryState,
};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::core::{GUID, HSTRING, w};

/// The three classic Windows power plans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerScheme {
    PowerSaver,
    Balanced,
    HighPerformance,
}

/// `(plan, GUID, label)`; the GUIDs are GUID_MAX / TYPICAL / MIN_POWER_SAVINGS.
const SCHEMES: [(PowerScheme, u128, &str); 3] = [
    (PowerScheme::PowerSaver, 0xa1841308_3541_4fab_bc81_f71556f20b4a, "Power saver"),
    (PowerScheme::Balanced, 0x381b4222_f694_41f0_9685_ff5bb260df2e, "Balanced"),
    (PowerScheme::HighPerformance, 0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c, "High performance"),
];

impl PowerScheme {
    fn guid(self) -> GUID {
        GUID::from_u128(SCHEMES[self as usize].1)
    }

    pub fn label(self) -> &'static str {
        SCHEMES[self as usize].2
    }
}

/// Active plan, or `None` when it is a custom / OEM plan we do not map.
pub fn active_scheme() -> Option<PowerScheme> {
    let mut ptr: *mut GUID = std::ptr::null_mut();
    // SAFETY: PowerGetActiveScheme allocates the GUID with LocalAlloc; we copy it, then free it.
    let guid = unsafe {
        if PowerGetActiveScheme(None, &mut ptr).is_err() || ptr.is_null() {
            return None;
        }
        let g = *ptr;
        LocalFree(Some(HLOCAL(ptr as *mut c_void)));
        g
    };
    SCHEMES.iter().find(|s| GUID::from_u128(s.1) == guid).map(|s| s.0)
}

/// Switch the active plan. Many Modern Standby laptops ship only "Balanced"; the error is
/// returned so the UI can explain it.
pub fn set_scheme(scheme: PowerScheme) -> Result<(), String> {
    // SAFETY: the GUID reference is valid for the duration of the call.
    unsafe { PowerSetActiveScheme(None, Some(&scheme.guid())) }
        .ok()
        .map_err(|e| format!("{} plan unavailable: {}", scheme.label(), e.message()))
}

/// Snapshot of the battery as Windows reports it.
#[derive(Clone, Debug, Default)]
pub struct BatteryStatus {
    pub ac_online: bool,
    pub percent: Option<u8>,
    pub charging: bool,
    /// Full-charge capacity right now (drops as the cells wear).
    pub full_charge_mwh: Option<u32>,
    /// Positive while charging, negative while discharging.
    pub rate_mw: Option<i32>,
}

/// `GetSystemPowerStatus` for the basics, `CallNtPowerInformation(SystemBatteryState)` for
/// capacity and charge rate.
pub fn battery_status() -> Option<BatteryStatus> {
    let mut sps = SYSTEM_POWER_STATUS::default();
    // SAFETY: plain out-parameter struct.
    unsafe { GetSystemPowerStatus(&mut sps) }.ok()?;
    let mut st = BatteryStatus {
        ac_online: sps.ACLineStatus == 1,
        percent: (sps.BatteryLifePercent <= 100).then_some(sps.BatteryLifePercent),
        charging: sps.BatteryFlag & 8 != 0,
        ..Default::default()
    };
    let mut sbs = SYSTEM_BATTERY_STATE::default();
    let size = size_of::<SYSTEM_BATTERY_STATE>() as u32;
    // SAFETY: the output buffer is a correctly sized SYSTEM_BATTERY_STATE.
    let status = unsafe { CallNtPowerInformation(SystemBatteryState, None, 0, Some(&mut sbs as *mut _ as *mut c_void), size) };
    if status.is_ok() && sbs.BatteryPresent {
        st.full_charge_mwh = (sbs.MaxCapacity > 0).then_some(sbs.MaxCapacity);
        st.rate_mw = (sbs.Rate != 0x8000_0000).then_some(sbs.Rate as i32);
        st.charging = sbs.Charging;
    }
    Some(st)
}

/// Identity strings the vendor detection works from.
#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub product: String,
    /// Marketing family, e.g. "Legion Pro 5 16IRX9" (Lenovo puts the machine type in `product`).
    pub family: String,
    pub bios_version: String,
}

impl DeviceInfo {
    /// Read `HKLM\HARDWARE\DESCRIPTION\System\BIOS`.
    pub fn read() -> Self {
        Self {
            manufacturer: bios_string("SystemManufacturer"),
            product: bios_string("SystemProductName"),
            family: bios_string("SystemFamily"),
            bios_version: bios_string("BIOSVersion"),
        }
    }

    /// Human model name: the family when it is more descriptive than the product code.
    pub fn model(&self) -> &str {
        if self.family.len() > self.product.len() { &self.family } else { &self.product }
    }
}

fn bios_string(value: &str) -> String {
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let key = w!(r"HARDWARE\DESCRIPTION\System\BIOS");
    let data = Some(buf.as_mut_ptr() as *mut c_void);
    // SAFETY: buffer and length describe the same allocation; RRF_RT_REG_SZ guarantees a terminator.
    if unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key, &HSTRING::from(value), RRF_RT_REG_SZ, None, data, Some(&mut len)) }.is_err() {
        return String::new();
    }
    let chars = (len as usize / 2).min(buf.len());
    String::from_utf16_lossy(&buf[..chars]).trim_end_matches('\0').trim().to_owned()
}

/// Lower-case executable names of all running processes (for per-app profiles).
pub fn running_processes() -> Vec<String> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed before returning; entry.dwSize is set as required.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let end = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            out.push(String::from_utf16_lossy(&entry.szExeFile[..end]).to_lowercase());
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        // Closing a snapshot cannot meaningfully fail; nothing to recover if it does.
        let _ = CloseHandle(snap);
    }
    out
}

/// Whole-system CPU utilisation from `GetSystemTimes` deltas.
#[derive(Default)]
pub struct CpuLoad {
    prev: Option<(u64, u64)>,
}

impl CpuLoad {
    /// Load in `0..=1` since the previous call (`None` on the first call).
    pub fn sample(&mut self) -> Option<f32> {
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: three valid out-parameters.
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        let (idle, total) = (ft(idle), ft(kernel) + ft(user)); // kernel time includes idle time
        let prev = self.prev.replace((idle, total))?;
        let (di, dt) = (idle.saturating_sub(prev.0), total.saturating_sub(prev.1));
        (dt > 0).then(|| 1.0 - di as f32 / dt as f32)
    }
}
