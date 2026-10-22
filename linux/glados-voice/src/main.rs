//! GLaDOS Voice for Linux: live voice changing, text to speech and a Melodyne-style note editor.

mod app;
mod audio;
mod selftest;
mod settings;
#[cfg(test)]
mod tests;
mod tts;
mod ui;

use adw::prelude::*;
use gtk4::gio;

pub const APP_ID: &str = "io.github.l0gic0l.GladosVoice";

fn main() -> gtk4::glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--selftest-live") {
        let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5.0);
        std::process::exit(selftest::live(seconds, args.get(3).map(String::as_str)));
    }
    let application = adw::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_OPEN).build();
    application.connect_activate(|a| {
        ui::window::build(a);
    });
    application.connect_open(|a, files, _| {
        let window = ui::window::build(a);
        if let Some(path) = files.first().and_then(|f| f.path()) {
            ui::window::open(&window, &path);
        }
    });
    application.run()
}
