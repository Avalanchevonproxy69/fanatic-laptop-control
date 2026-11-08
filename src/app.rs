//! The window: sidebar navigation, four screens and the mode-switch overlay.

use eframe::egui::{self, Align2, Key, RichText, Stroke, Ui};

use crate::backend::{Sensors, Vendor};
use crate::battery;
use crate::curve::{CurveSet, Fan, Mode};
use crate::profiles::{self, Reason};
use crate::settings::Settings;
use crate::system::DeviceInfo;
use crate::theme::{self, BASE, FAINT, LINE, MUTED, SELECTED, SURFACE, TEXT};
use crate::widgets;
use crate::worker::{self, Command, Event, Telemetry, Worker};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Overview,
    Curves,
    Battery,
    Profiles,
}

const SCREENS: [(Screen, &str, Key); 4] = [
    (Screen::Overview, "Overview", Key::Num1),
    (Screen::Curves, "Fans", Key::Num2),
    (Screen::Battery, "Battery", Key::Num3),
    (Screen::Profiles, "Profiles", Key::Num4),
];

/// How long the mode overlay stays visible, in seconds (notices stay twice as long).
const OSD_SECONDS: f64 = 2.2;

pub struct App {
    settings: Settings,
    worker: Worker,
    screen: Screen,
    device: Option<(DeviceInfo, String)>,
    tel: Option<Telemetry>,
    active: Mode,
    reason: Reason,
    fan: Fan,
    edit_mode: Mode,
    drag: Option<usize>,
    osd: Option<(Mode, String, f64)>,
    notice: Option<(String, f64)>,
}

fn label(ui: &mut Ui, text: impl Into<String>, font: egui::FontId, color: egui::Color32) -> egui::Response {
    ui.label(RichText::new(text).font(font).color(color))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let settings = Settings::load();
        let worker = worker::spawn(&settings, cc.egui_ctx.clone());
        let mode = settings.mode;
        Self {
            settings,
            worker,
            screen: Screen::Overview,
            device: None,
            tel: None,
            active: mode,
            reason: Reason::Manual,
            fan: Fan::Cpu,
            edit_mode: mode,
            drag: None,
            osd: None,
            notice: None,
        }
    }

    fn send(&mut self, cmd: Command, now: f64) {
        if self.worker.tx.send(cmd).is_err() {
            self.notice = Some(("Background worker stopped".into(), now));
        }
    }

    fn persist(&mut self, now: f64) {
        if let Err(e) = self.settings.save() {
            self.notice = Some((format!("Could not save settings: {e}"), now));
        }
    }

    fn pick_mode(&mut self, mode: Mode, now: f64) {
        self.settings.mode = mode;
        self.send(Command::SetMode(mode), now);
        self.persist(now);
    }

    fn drain_events(&mut self, now: f64) {
        while let Ok(ev) = self.worker.rx.try_recv() {
            match ev {
                Event::Ready { device, backend } => self.device = Some((device, backend)),
                Event::Telemetry(t) => {
                    if let Some(mwh) = t.battery.as_ref().and_then(|b| b.full_charge_mwh)
                        && battery::record(&mut self.settings.capacity_history, battery::today(), mwh)
                    {
                        self.persist(now);
                    }
                    self.tel = Some(*t);
                }
                Event::ModeChanged { mode, reason, errors } => {
                    self.active = mode;
                    if self.settings.show_overlay {
                        self.osd = Some((mode, reason.describe(), now));
                    }
                    self.reason = reason;
                    if !errors.is_empty() {
                        self.notice = Some((errors.join("  \u{b7}  "), now));
                    }
                }
                Event::Notice(text) => self.notice = Some((text, now)),
            }
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        // Device row: model on top, vendor and the firmware interface underneath.
        let (model, detail) = match &self.device {
            Some((dev, _)) => {
                let vendor = Vendor::from_manufacturer(&dev.manufacturer);
                (dev.model().to_owned(), format!("{} \u{b7} BIOS {}", vendor.native_interface(), dev.bios_version))
            }
            None => ("Detecting...".to_owned(), String::new()),
        };
        label(ui, model, theme::text(13.0), TEXT).on_hover_text(&detail);
        label(ui, self.device.as_ref().map_or("", |d| d.1.as_str()), theme::text(11.5), FAINT);
        ui.add_space(14.0);
        for (screen, name, _) in SCREENS {
            if widgets::nav_item(ui, name, self.screen == screen).clicked() {
                self.screen = screen;
            }
        }
    }

    /// Bottom status bar: live readings on the left, power state on the right.
    fn status_bar(&self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            if let Some(s) = self.tel.as_ref().map(|t| &t.sensors) {
                label(ui, format!("CPU {:.0} \u{b0}C", s.cpu_temp), theme::text(11.5), FAINT);
                label(ui, format!("GPU {:.0} \u{b0}C", s.gpu_temp), theme::text(11.5), FAINT);
                label(ui, format!("Fans {:.0} / {:.0} rpm", s.cpu_fan_rpm, s.gpu_fan_rpm), theme::text(11.5), FAINT);
            }
            label(ui, format!("Mode {} ({})", self.active.label(), self.reason.describe()), theme::text(11.5), FAINT);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(b) = self.tel.as_ref().and_then(|t| t.battery.as_ref()) {
                    let pct = b.percent.map_or("--".into(), |p| format!("{p}%"));
                    let src = if b.ac_online { "AC" } else { "Battery" };
                    label(ui, format!("{src} \u{b7} {pct} \u{b7} limit {}%", self.settings.charge_limit), theme::text(11.5), FAINT);
                }
            });
        });
    }

    fn header(&mut self, ui: &mut Ui, title: &str, now: f64) {
        ui.horizontal(|ui| {
            label(ui, title, theme::display(20.0), TEXT);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let items: Vec<(Mode, &str)> = Mode::ALL.iter().rev().map(|m| (*m, m.label())).collect();
                if let Some(m) = widgets::segmented(ui, &items, self.active) {
                    self.pick_mode(m, now);
                }
            });
        });
        ui.add_space(14.0);
    }

    fn overview(&mut self, ui: &mut Ui, now: f64) {
        self.header(ui, "Overview", now);
        let s = self.tel.as_ref().map(|t| t.sensors.clone()).unwrap_or_default();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 250.0);
                widgets::thermal_map(ui, &s, now);
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                let rpm = |v: f32| format!("{:.0}", (v / 10.0).round() * 10.0);
                widgets::numeral(ui, &rpm(s.cpu_fan_rpm), "RPM", "CPU fan");
                widgets::numeral(ui, &rpm(s.gpu_fan_rpm), "RPM", "GPU fan");
                let watts = format!("{:.0}", s.cpu_power_w + s.gpu_power_w);
                widgets::numeral(ui, &watts, "W", "Package power");
                if let Some(scheme) = self.tel.as_ref().and_then(|t| t.scheme) {
                    label(ui, format!("Windows plan: {}", scheme.label()), theme::text(12.0), FAINT);
                }
            });
        });
        ui.add_space(16.0);
        widgets::caption(ui, "Sensors");
        egui::Grid::new("sensors").num_columns(4).spacing([40.0, 6.0]).show(ui, |ui| {
            for (i, (name, value, unit)) in sensor_rows(&s).into_iter().enumerate() {
                label(ui, name, theme::mono(13.0), MUTED);
                label(ui, format!("{value:>7.1} {unit}"), theme::mono(13.0), TEXT);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    }

    fn curves(&mut self, ui: &mut Ui, now: f64) {
        self.header(ui, "Fans", now);
        let (mode, fan) = (self.edit_mode, self.fan);
        ui.horizontal(|ui| {
            let modes: Vec<(Mode, &str)> = Mode::ALL.iter().map(|m| (*m, m.label())).collect();
            self.edit_mode = widgets::segmented(ui, &modes, mode).unwrap_or(mode);
            ui.add_space(12.0);
            self.fan = widgets::segmented(ui, &[(Fan::Cpu, "CPU fan"), (Fan::Gpu, "GPU fan")], fan).unwrap_or(fan);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Reset").clicked() {
                    self.settings.curves.insert(mode, CurveSet::preset(mode));
                    self.send(Command::Curves(mode, CurveSet::preset(mode)), now);
                    self.persist(now);
                }
            });
        });
        ui.add_space(12.0);
        let live = self.tel.as_ref().map_or(45.0, |t| if fan == Fan::Cpu { t.sensors.cpu_temp } else { t.sensors.gpu_temp });
        let mut set = self.settings.curves_for(mode);
        let was_dragging = self.drag.is_some();
        if widgets::curve_editor(ui, set.get_mut(fan), live, &mut self.drag) {
            self.settings.curves.insert(mode, set.clone());
            self.send(Command::Curves(mode, set.clone()), now);
        }
        if was_dragging && self.drag.is_none() {
            self.persist(now);
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            for pt in &set.get(fan).points {
                label(ui, format!("{:>3.0}\u{b0} {:>3.0}%", pt[0], pt[1]), theme::mono(12.0), theme::temp_color(pt[0]));
            }
        });
        label(
            ui,
            "Drag a point. Temperatures stay ordered and duty never falls as heat rises, as firmware requires.",
            theme::text(12.0),
            FAINT,
        );
    }

    fn battery(&mut self, ui: &mut Ui, now: f64) {
        self.header(ui, "Battery", now);
        widgets::caption(ui, "Charge limit");
        let limits = [(60u8, "60%  desk"), (80u8, "80%  recommended"), (100u8, "100%  travel")];
        if let Some(p) = widgets::segmented(ui, &limits, self.settings.charge_limit) {
            self.settings.charge_limit = p;
            self.send(Command::SetChargeLimit(p), now);
            self.persist(now);
        }
        ui.add_space(18.0);
        let bat = self.tel.as_ref().and_then(|t| t.battery.clone()).unwrap_or_default();
        let history = &self.settings.capacity_history;
        let fc = battery::forecast(history, self.settings.design_capacity_mwh);
        let fmt = |v: Option<f32>, digits: usize| v.map_or("--".to_owned(), |v| format!("{v:.digits$}"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 48.0;
            let state = if bat.charging { "Charging" } else { "Charge" };
            widgets::numeral(ui, &fmt(bat.percent.map(f32::from), 0), "%", state);
            widgets::numeral(ui, &fmt(fc.map(|f| f.health * 100.0), 1), "%", "Health");
            widgets::numeral(ui, &fmt(bat.full_charge_mwh.map(|m| m as f32 / 1000.0), 1), "Wh", "Full charge");
            widgets::numeral(ui, &fmt(bat.rate_mw.map(|r| r as f32 / 1000.0), 1), "W", "Rate");
        });
        ui.add_space(14.0);
        let line = match fc.and_then(|f| f.months_to_worn.map(|m| (m, f.loss_per_month))) {
            Some((m, loss)) => {
                format!("At this rate your battery reaches 80% capacity in ~{m:.0} months ({:.2}% per month).", loss * 100.0)
            }
            None if history.len() < 2 => "The forecast starts after a week of daily capacity samples.".into(),
            None => "Capacity is stable: no measurable wear yet.".into(),
        };
        label(ui, line, theme::text(15.0), TEXT);
        label(ui, format!("{} daily samples recorded", history.len()), theme::mono(11.0), FAINT);
    }

    fn profiles(&mut self, ui: &mut Ui, now: f64) {
        self.header(ui, "Profiles", now);
        let mut changed = ui.checkbox(&mut self.settings.automation, "Switch modes automatically for these apps").changed();
        ui.add_space(10.0);
        widgets::caption(ui, "Per-app profiles  (edit the list in settings.json)");
        egui::Grid::new("rules").num_columns(3).spacing([28.0, 8.0]).show(ui, |ui| {
            for (i, rule) in self.settings.rules.iter_mut().enumerate() {
                label(ui, &rule.label, theme::text(14.0), TEXT);
                label(ui, &rule.exe, theme::mono(12.0), MUTED);
                egui::ComboBox::from_id_salt(("rule", i)).selected_text(rule.mode.label()).show_ui(ui, |ui| {
                    for m in Mode::ALL {
                        changed |= ui.selectable_value(&mut rule.mode, m, m.label()).changed();
                    }
                });
                ui.end_row();
            }
        });
        ui.add_space(22.0);
        widgets::caption(ui, "Silent hours");
        let sh = &mut self.settings.silent_hours;
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut sh.enabled, "Keep fans in Silent from").changed();
            let clock = |v: f64, _: std::ops::RangeInclusive<usize>| format!("{:02}:{:02}", v as u16 / 60, v as u16 % 60);
            changed |= ui.add(egui::DragValue::new(&mut sh.start_min).range(0..=1410).speed(5).custom_formatter(clock)).changed();
            ui.label("to");
            changed |= ui.add(egui::DragValue::new(&mut sh.end_min).range(0..=1410).speed(5).custom_formatter(clock)).changed();
        });
        let inside = self.settings.silent_hours.contains(profiles::local_minute_of_day());
        label(ui, if inside { "Silent hours are active now." } else { "Outside Silent hours." }, theme::text(12.0), FAINT);
        if changed {
            let s = &self.settings;
            self.send(Command::Automation { enabled: s.automation, rules: s.rules.clone(), silent: s.silent_hours }, now);
            self.persist(now);
        }
    }

    fn overlays(&mut self, ctx: &egui::Context, now: f64) {
        if let Some((mode, why, at)) = self.osd.clone() {
            let age = now - at;
            let alpha = ((OSD_SECONDS - age) / 0.3).clamp(0.0, 1.0) as f32;
            egui::Area::new(egui::Id::unique("osd"))
                .order(egui::Order::Foreground)
                .anchor(Align2::CENTER_BOTTOM, [0.0, -48.0])
                .show(ctx, |ui| {
                    let frame =
                        egui::Frame::new().fill(SURFACE.gamma_multiply(alpha)).stroke(Stroke::new(1.0, LINE.gamma_multiply(alpha)));
                    frame.corner_radius(8).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                        ui.set_min_width(250.0);
                        label(ui, mode.label(), theme::text(14.0), TEXT.gamma_multiply(alpha));
                        label(ui, why, theme::text(12.0), MUTED.gamma_multiply(alpha));
                    });
                });
            if age > OSD_SECONDS {
                self.osd = None;
            }
            ctx.request_repaint();
        }
        if let Some((text, at)) = self.notice.clone() {
            egui::Area::new(egui::Id::unique("notice"))
                .order(egui::Order::Foreground)
                .anchor(Align2::RIGHT_BOTTOM, [-20.0, -20.0])
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(SELECTED)
                        .corner_radius(8)
                        .inner_margin(12)
                        .show(ui, |ui| label(ui, text, theme::text(13.0), TEXT));
                });
            if now - at > OSD_SECONDS * 2.0 {
                self.notice = None;
            }
            ctx.request_repaint();
        }
    }
}

/// `(name, value, unit)` rows of the sensor table, two per grid row.
fn sensor_rows(s: &Sensors) -> [(&'static str, f32, &'static str); 8] {
    [
        ("CPU package", s.cpu_temp, "\u{b0}C"),
        ("CPU fan", s.cpu_fan_rpm, "rpm"),
        ("GPU core", s.gpu_temp, "\u{b0}C"),
        ("GPU fan", s.gpu_fan_rpm, "rpm"),
        ("VRM", s.vrm_temp, "\u{b0}C"),
        ("Total power", s.cpu_power_w + s.gpu_power_w, "W"),
        ("NVMe SSD", s.ssd_temp, "\u{b0}C"),
        ("CPU load", s.cpu_load * 100.0, "%"),
    ]
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.drain_events(now);
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(Key::F5)) {
                self.pick_mode(self.active.next(), now);
            }
            if let Some((screen, ..)) = SCREENS.iter().find(|(_, _, k)| ctx.input(|i| i.key_pressed(*k))) {
                self.screen = *screen;
            }
        }
        let chrome = |m: egui::Margin| egui::Frame::new().fill(theme::CHROME).inner_margin(m);
        let bar = egui::Panel::bottom("status").exact_size(26.0).resizable(false).frame(chrome(egui::Margin::symmetric(12, 0)));
        bar.show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("nav")
            .exact_size(220.0)
            .resizable(false)
            .frame(chrome(egui::Margin::symmetric(8, 14)))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BASE).inner_margin(20)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match self.screen {
                Screen::Overview => self.overview(ui, now),
                Screen::Curves => self.curves(ui, now),
                Screen::Battery => self.battery(ui, now),
                Screen::Profiles => self.profiles(ui, now),
            });
        });
        self.overlays(&ctx, now);
        if self.screen == Screen::Overview {
            ctx.request_repaint(); // spinning fans
        }
    }

    fn on_exit(&mut self) {
        // Nowhere left to report a failure at shutdown; the previous file stays intact.
        let _ = self.settings.save();
    }
}
