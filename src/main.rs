#![windows_subsystem = "windows"]
//! Fanatic - one portable control center for gaming and creator laptops on Windows.
//!
//! Fan curves, power modes and the battery charge limit for ASUS, Lenovo Legion, HP Omen,
//! MSI, Dell / Alienware, Acer, Gigabyte and Framework machines, behind one
//! [`backend::VendorBackend`] trait that is picked automatically from the BIOS strings.

mod app;
mod backend;
mod battery;
mod curve;
mod profiles;
mod settings;
mod system;
mod theme;
mod widgets;
mod worker;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Fanatic")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([1060.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native("Fanatic", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
