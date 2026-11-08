//! Background thread that owns the vendor backend. The UI talks to it only through channels,
//! so slow firmware calls (WMI, ACPI, EC) never stall a frame.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::backend::{self, Sensors, VendorBackend};
use crate::curve::{CurveSet, Fan, Mode};
use crate::profiles::{self, AppRule, Reason, SilentHours};
use crate::settings::Settings;
use crate::system::{self, BatteryStatus, DeviceInfo, PowerScheme};

const TELEMETRY_EVERY: Duration = Duration::from_millis(1000);
const PROFILES_EVERY: Duration = Duration::from_secs(3);

pub enum Command {
    /// The user picked a mode by hand.
    SetMode(Mode),
    /// Curves of one mode were edited; re-uploaded if that mode is active.
    Curves(Mode, CurveSet),
    SetChargeLimit(u8),
    Automation {
        enabled: bool,
        rules: Vec<AppRule>,
        silent: SilentHours,
    },
}

pub struct Telemetry {
    pub sensors: Sensors,
    pub battery: Option<BatteryStatus>,
    pub scheme: Option<PowerScheme>,
}

pub enum Event {
    Ready { device: DeviceInfo, backend: String },
    Telemetry(Box<Telemetry>),
    ModeChanged { mode: Mode, reason: Reason, errors: Vec<String> },
    Notice(String),
}

pub struct Worker {
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
}

struct State {
    backend: Box<dyn VendorBackend>,
    active: Mode,
    manual: Mode,
    curves: BTreeMap<Mode, CurveSet>,
    automation: bool,
    rules: Vec<AppRule>,
    silent: SilentHours,
    /// Last automatic decision; switching is edge-triggered so manual picks are respected.
    last_auto: Option<(Mode, Reason)>,
}

impl State {
    fn upload_curves(&mut self, mode: Mode) -> Vec<String> {
        let set = self.curves.get(&mode).cloned().unwrap_or_else(|| CurveSet::preset(mode));
        [Fan::Cpu, Fan::Gpu].into_iter().filter_map(|f| self.backend.fan_curve(f, set.get(f)).err()).collect()
    }

    /// Vendor mode, both fan curves, then the matching Windows power plan.
    fn apply_mode(&mut self, mode: Mode) -> Vec<String> {
        let mut errors: Vec<String> = self.backend.set_mode(mode).err().into_iter().collect();
        errors.extend(self.upload_curves(mode));
        errors.extend(system::set_scheme(mode.power_scheme()).err());
        self.active = mode;
        errors
    }

    /// Re-evaluate per-app rules and Silent hours; returns a mode change to report.
    fn evaluate(&mut self) -> Option<Event> {
        if !self.automation {
            self.last_auto = None;
            return None;
        }
        let running = system::running_processes();
        let decision = profiles::resolve(&self.rules, &running, &self.silent, profiles::local_minute_of_day(), self.manual);
        if self.last_auto.as_ref() == Some(&decision) {
            return None;
        }
        // The first evaluation only records a baseline: launching Fanatic never flips modes.
        let first = self.last_auto.replace(decision.clone()).is_none();
        if first || decision.0 == self.active {
            return None;
        }
        let errors = self.apply_mode(decision.0);
        Some(Event::ModeChanged { mode: decision.0, reason: decision.1, errors })
    }
}

/// Start the worker. `ctx` is used to wake the UI when new data arrives.
pub fn spawn(settings: &Settings, ctx: egui::Context) -> Worker {
    let (tx, cmd_rx) = mpsc::channel::<Command>();
    let (ev_tx, rx) = mpsc::channel::<Event>();
    let s = settings.clone();
    let body = move || {
        let send = |e: Event| {
            let ok = ev_tx.send(e).is_ok();
            ctx.request_repaint();
            ok
        };
        let device = DeviceInfo::read();
        let backend = backend::detect(&device);
        let (rules, silent, automation) = (s.rules, s.silent_hours, s.automation);
        let mut st =
            State { backend, active: s.mode, manual: s.mode, curves: s.curves, automation, rules, silent, last_auto: None };
        // Restore the firmware-side state we own, without touching the Windows power plan.
        let mut startup: Vec<String> = st.backend.set_mode(s.mode).err().into_iter().collect();
        startup.extend(st.backend.set_charge_limit(s.charge_limit).err());
        startup.extend(st.upload_curves(s.mode));
        send(Event::Ready { device, backend: st.backend.name() });
        startup.into_iter().for_each(|e| _ = send(Event::Notice(e)));
        let (mut next_tel, mut next_prof) = (Instant::now(), Instant::now());
        loop {
            match cmd_rx.recv_timeout(next_tel.min(next_prof).saturating_duration_since(Instant::now())) {
                Ok(Command::SetMode(m)) => {
                    st.manual = m;
                    let errors = st.apply_mode(m);
                    send(Event::ModeChanged { mode: m, reason: Reason::Manual, errors });
                }
                Ok(Command::Curves(m, set)) => {
                    st.curves.insert(m, set);
                    if m == st.active {
                        st.upload_curves(m).into_iter().for_each(|e| _ = send(Event::Notice(e)));
                    }
                }
                Ok(Command::SetChargeLimit(p)) => {
                    let msg = st.backend.set_charge_limit(p).map_or_else(|e| e, |()| format!("Charging now stops at {p}%"));
                    send(Event::Notice(msg));
                    send(Event::Ready { device: DeviceInfo::read(), backend: st.backend.name() });
                }
                Ok(Command::Automation { enabled, rules, silent }) => {
                    (st.automation, st.rules, st.silent) = (enabled, rules, silent);
                    next_prof = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            if now >= next_tel {
                next_tel = now + TELEMETRY_EVERY;
                let tel = Telemetry {
                    sensors: st.backend.read_sensors(),
                    battery: system::battery_status(),
                    scheme: system::active_scheme(),
                };
                if !send(Event::Telemetry(Box::new(tel))) {
                    break;
                }
            }
            if now >= next_prof {
                next_prof = now + PROFILES_EVERY;
                if let Some(ev) = st.evaluate() {
                    send(ev);
                }
            }
        }
    };
    if let Err(e) = std::thread::Builder::new().name("fanatic-worker".into()).spawn(body) {
        // The UI still renders without a worker; explain why nothing updates.
        let (dead_tx, dead_rx) = mpsc::channel();
        let _ = dead_tx.send(Event::Notice(format!("Worker thread failed to start: {e}")));
        return Worker { tx, rx: dead_rx };
    }
    Worker { tx, rx }
}
