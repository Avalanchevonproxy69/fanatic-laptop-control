//! Custom-painted widgets: the chassis thermal map, the thermal-gradient fan curve editor
//! and small pieces of chrome.

use eframe::egui::{self, Align2, Color32, Mesh, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::backend::Sensors;
use crate::curve::{FanCurve, TEMP_MAX, TEMP_MIN};
use crate::theme::{self, FAINT, LINE, MUTED, SELECTED, SURFACE, TEXT};

/// Group header above a block of rows ("Charging", "Sensors"), like Windows 11 Settings.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::text(13.0)).color(TEXT).strong());
}

/// Segmented control. Returns the newly clicked item.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, items: &[(T, &str)], current: T) -> Option<T> {
    let mut picked = None;
    let frame = egui::Frame::new().fill(theme::CHROME).stroke(Stroke::new(1.0, LINE)).corner_radius(6).inner_margin(2);
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (value, label) in items {
                let on = *value == current;
                let text = egui::RichText::new(*label).font(theme::text(12.5)).color(if on { TEXT } else { MUTED });
                let fill = if on { SELECTED } else { Color32::TRANSPARENT };
                if ui.add(egui::Button::new(text).fill(fill).stroke(Stroke::NONE)).clicked() && !on {
                    picked = Some(*value);
                }
            }
        });
    });
    picked
}

/// Sidebar navigation row; the selected row gets a short accent pill on the left edge.
pub fn nav_item(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let p = ui.painter();
    if selected || resp.hovered() {
        p.rect_filled(rect, 5, if selected { SELECTED } else { SURFACE });
    }
    if selected {
        p.rect_filled(Rect::from_center_size(pos2(rect.left() + 1.5, rect.center().y), vec2(3.0, 16.0)), 2, theme::ACCENT);
    }
    let color = if selected { TEXT } else { MUTED };
    p.text(pos2(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, label, theme::text(13.5), color);
    resp
}

/// A labelled value: small grey label above an SF Pro Display number and its unit.
pub fn numeral(ui: &mut Ui, value: &str, unit: &str, label: &str) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).font(theme::text(12.0)).color(FAINT));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(value).font(theme::display(26.0)).color(TEXT));
            ui.label(egui::RichText::new(unit).font(theme::text(12.0)).color(FAINT));
        });
    });
}

/// Top-down thermal image of the laptop base: CPU and GPU dies, VRM, SSD, heat pipes and
/// spinning fans, colored with the ironbow ramp.
pub fn thermal_map(ui: &mut Ui, s: &Sensors, time: f64) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, (w * 0.56).min(430.0)), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 10, theme::CHROME);
    let body = rect.shrink2(vec2(rect.width() * 0.07, rect.height() * 0.09));
    let at = |u: f32, v: f32| body.lerp_inside(vec2(u, v));
    let base = 29.0 + (s.cpu_temp + s.gpu_temp - 80.0).max(0.0) * 0.08;
    // (u, v, sigma, temperature) heat sources in chassis coordinates.
    let mut src = vec![
        (0.40, 0.30, 0.085, s.cpu_temp),
        (0.62, 0.31, 0.1, s.gpu_temp),
        (0.51, 0.16, 0.05, s.vrm_temp),
        (0.30, 0.66, 0.06, s.ssd_temp),
    ];
    for (u0, v0, u1, v1, temp) in [(0.40, 0.30, 0.13, 0.17, s.cpu_temp), (0.62, 0.31, 0.87, 0.17, s.gpu_temp)] {
        for k in 1..=6 {
            let f = k as f32 / 7.0; // heat pipe: a chain of small sources from die to fan
            src.push((u0 + (u1 - u0) * f, v0 + (v1 - v0) * f, 0.035, base + (temp - base) * (0.78 - 0.4 * f)));
        }
    }
    let field = |u: f32, v: f32| {
        src.iter().fold(base, |acc, &(su, sv, sg, st)| {
            let (du, dv) = ((u - su) * 1.6, v - sv);
            acc.max(base + (st - base) * (-(du * du + dv * dv) / (2.0 * sg * sg)).exp())
        })
    };
    let (nx, ny) = (64u32, 36u32);
    let mut mesh = Mesh::default();
    for j in 0..=ny {
        for i in 0..=nx {
            let (u, v) = (i as f32 / nx as f32, j as f32 / ny as f32);
            mesh.colored_vertex(at(u, v), theme::temp_color(field(u, v)));
            if i < nx && j < ny {
                let a = j * (nx + 1) + i;
                mesh.add_triangle(a, a + 1, a + nx + 1);
                mesh.add_triangle(a + 1, a + nx + 2, a + nx + 1);
            }
        }
    }
    p.add(Shape::mesh(mesh));
    let ghost = Stroke::new(1.0, Color32::from_white_alpha(28));
    p.rect_stroke(body, 14, Stroke::new(1.5, Color32::from_white_alpha(70)), StrokeKind::Outside);
    let (kw, kh) = (body.width() * 0.8 / 15.0, body.height() * 0.36 / 6.0);
    for k in 0..90 {
        let min = at(0.1, 0.42) + vec2((k % 15) as f32 * kw, (k / 15) as f32 * kh);
        p.rect_stroke(Rect::from_min_size(min, vec2(kw, kh)).shrink(2.0), 2, ghost, StrokeKind::Inside);
    }
    p.rect_stroke(Rect::from_min_max(at(0.36, 0.82), at(0.64, 0.97)), 6, ghost, StrokeKind::Inside);
    // Fans: blades turn at a visually slowed rate proportional to RPM.
    for (c, rpm) in [(at(0.13, 0.17), s.cpu_fan_rpm), (at(0.87, 0.17), s.gpu_fan_rpm)] {
        let r = body.height() * 0.13;
        p.circle_stroke(c, r, Stroke::new(1.2, Color32::from_white_alpha(90)));
        let spin = (time * rpm as f64 / 900.0) as f32;
        for b in 0..9 {
            let a = spin + b as f32 * std::f32::consts::TAU / 9.0;
            let (bend, tip) = (c + egui::Vec2::angled(a + 0.5) * r * 0.45, c + egui::Vec2::angled(a) * r * 0.9);
            p.add(Shape::line(vec![c, bend, tip], Stroke::new(1.0, Color32::from_white_alpha(60))));
        }
        p.circle_filled(c, r * 0.22, theme::CHROME);
    }
    for (label, c, temp) in [("CPU", at(0.40, 0.30), s.cpu_temp), ("GPU", at(0.62, 0.31), s.gpu_temp)] {
        let cross = Stroke::new(1.0, Color32::from_white_alpha(200));
        p.line_segment([c - vec2(9.0, 0.0), c + vec2(9.0, 0.0)], cross);
        p.line_segment([c - vec2(0.0, 9.0), c + vec2(0.0, 9.0)], cross);
        let galley = p.layout_no_wrap(format!("{label} {temp:.1}\u{b0}"), theme::mono(12.0), Color32::WHITE);
        let r = Align2::LEFT_BOTTOM.anchor_size(c + vec2(14.0, -14.0), galley.size());
        p.rect_filled(r.expand(4.0), 4, Color32::from_black_alpha(150));
        p.galley(r.min, galley, Color32::WHITE);
    }
    // Vertical ironbow legend.
    let bar = Rect::from_min_size(pos2(rect.right() - 26.0, rect.top() + 24.0), vec2(8.0, rect.height() - 48.0));
    let mut legend = Mesh::default();
    for k in 0..=16u32 {
        let (t, y) = (k as f32 / 16.0, bar.bottom() - bar.height() * k as f32 / 16.0);
        legend.colored_vertex(pos2(bar.left(), y), theme::ironbow(t));
        legend.colored_vertex(pos2(bar.right(), y), theme::ironbow(t));
        if k > 0 {
            legend.add_triangle(k * 2 - 2, k * 2 - 1, k * 2);
            legend.add_triangle(k * 2 - 1, k * 2 + 1, k * 2);
        }
    }
    p.add(Shape::mesh(legend));
    for c in [30.0, 60.0, 90.0] {
        let y = bar.bottom() - bar.height() * (c - theme::SCALE_MIN_C) / (theme::SCALE_MAX_C - theme::SCALE_MIN_C);
        p.text(pos2(bar.left() - 4.0, y), Align2::RIGHT_CENTER, format!("{c:.0}"), theme::mono(10.0), MUTED);
    }
}

/// Fan curve editor. The area under the curve is filled with the ironbow color of each
/// temperature; points are dragged with the mouse. Returns true when the curve changed.
pub fn curve_editor(ui: &mut Ui, curve: &mut FanCurve, live_temp: f32, drag: &mut Option<usize>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 360.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 10, theme::CHROME);
    let plot = Rect::from_min_max(rect.min + vec2(48.0, 18.0), rect.max - vec2(18.0, 30.0));
    let span = TEMP_MAX - TEMP_MIN;
    let to_screen =
        |t: f32, d: f32| pos2(plot.left() + (t - TEMP_MIN) / span * plot.width(), plot.bottom() - d / 100.0 * plot.height());
    for k in 0..=7 {
        let t = TEMP_MIN + k as f32 * 10.0;
        let x = to_screen(t, 0.0).x;
        p.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.0, LINE));
        p.text(pos2(x, plot.bottom() + 8.0), Align2::CENTER_TOP, format!("{t:.0}\u{b0}"), theme::mono(11.0), FAINT);
    }
    for k in 0..=5 {
        let (d, y) = (k as f32 * 20.0, to_screen(TEMP_MIN, k as f32 * 20.0).y);
        p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, LINE));
        p.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{d:.0}%"), theme::mono(11.0), FAINT);
    }
    // Thermal fill: one vertical strip per degree, bright at the curve and fading to the floor.
    let steps = span as u32;
    let mut mesh = Mesh::default();
    for k in 0..=steps {
        let t = TEMP_MIN + k as f32;
        let col = theme::temp_color(t);
        mesh.colored_vertex(to_screen(t, curve.duty_at(t)), col.gamma_multiply(0.85));
        mesh.colored_vertex(to_screen(t, 0.0), col.gamma_multiply(0.08));
        if k > 0 {
            mesh.add_triangle(k * 2 - 2, k * 2 - 1, k * 2);
            mesh.add_triangle(k * 2 - 1, k * 2 + 1, k * 2);
        }
    }
    p.add(Shape::mesh(mesh));
    let line = (0..=steps).map(|k| TEMP_MIN + k as f32).map(|t| to_screen(t, curve.duty_at(t))).collect();
    p.add(Shape::line(line, Stroke::new(2.0, TEXT)));

    let mut changed = false;
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    if resp.drag_started()
        && let Some(q) = pointer
    {
        *drag = curve.points.iter().position(|pt| to_screen(pt[0], pt[1]).distance(q) < 16.0);
    }
    if resp.dragged()
        && let (Some(i), Some(q)) = (*drag, pointer)
    {
        let before = curve.points[i];
        let t = TEMP_MIN + (q.x - plot.left()) / plot.width() * span;
        curve.set_point(i, t, (plot.bottom() - q.y) / plot.height() * 100.0);
        changed = curve.points[i] != before;
    }
    if resp.drag_stopped() {
        *drag = None;
    }
    for (i, pt) in curve.points.iter().enumerate() {
        let c = to_screen(pt[0], pt[1]);
        let hot = *drag == Some(i) || pointer.is_some_and(|q| c.distance(q) < 16.0);
        let r = if hot { 8.0 } else { 6.0 };
        p.circle_filled(c, r, if hot { theme::ACCENT } else { theme::SURFACE });
        p.circle_stroke(c, r, Stroke::new(2.0, TEXT));
        if hot {
            p.text(
                c - vec2(0.0, 16.0),
                Align2::CENTER_BOTTOM,
                format!("{:.0}\u{b0}C  {:.0}%", pt[0], pt[1]),
                theme::mono(12.0),
                TEXT,
            );
        }
    }
    // Live operating point.
    let t = live_temp.clamp(TEMP_MIN, TEMP_MAX);
    let now = to_screen(t, curve.duty_at(t));
    p.line_segment([pos2(now.x, plot.top()), pos2(now.x, plot.bottom())], Stroke::new(1.0, MUTED));
    p.circle_filled(now, 4.5, TEXT);
    let tag = format!("now {live_temp:.0}\u{b0} \u{2192} {:.0}%", curve.duty_at(t));
    p.text(pos2(now.x + 8.0, plot.top() + 4.0), Align2::LEFT_TOP, tag, theme::text(11.5), MUTED);
    changed
}
