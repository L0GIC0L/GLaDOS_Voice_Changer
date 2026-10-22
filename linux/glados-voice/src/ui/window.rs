//! The main window: a view switcher over the Live, Speak, Editor and Voice pages.

use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk4::{gio, glib, Orientation};

use super::{editor_page, live_page, speak_page, style, voice_page};
use crate::app::{App, Change};
use crate::settings::ColorScheme;

/// Builds and shows the main window, or brings the existing one forward.
pub fn build(application: &adw::Application) -> gtk4::Window {
    if let Some(w) = application.active_window() {
        w.present();
        return w;
    }
    let app = App::new();
    {
        let s = app.settings.borrow();
        style::apply(s.color_scheme, s.aperture_accent);
    }

    gtk4::Window::set_default_icon_name(crate::APP_ID);
    let window = adw::ApplicationWindow::builder().application(application).title("GLaDOS Voice").default_width(980).default_height(880).build();
    let stack = adw::ViewStack::new();
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    let header = adw::HeaderBar::builder().title_widget(&switcher).build();
    header.pack_end(&appearance_menu(&app));

    let go_editor = {
        let stack = stack.clone();
        move || stack.set_visible_child_name("editor")
    };
    let go_speak = {
        let stack = stack.clone();
        move || stack.set_visible_child_name("speak")
    };
    stack.add_titled_with_icon(&live_page::build(&app), Some("live"), "Live", "audio-input-microphone-symbolic");
    stack.add_titled_with_icon(&speak_page::build(&app, &window, go_editor), Some("speak"), "Speak", "audio-volume-high-symbolic");
    stack.add_titled_with_icon(&editor_page::build(&app, &window, go_speak), Some("editor"), "Editor", "document-edit-symbolic");
    stack.add_titled_with_icon(&voice_page::build(&app), Some("voice"), "Voice", "emblem-system-symbolic");
    // GLADOS_VOICE_PAGE=speak|editor|voice opens on another tab (handy for screenshots and tests).
    let start = std::env::var("GLADOS_VOICE_PAGE").unwrap_or_default();
    stack.set_visible_child_name(if ["speak", "editor", "voice"].contains(&start.as_str()) { &start } else { "live" });

    let banner = adw::Banner::new("");
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&banner);
    toolbar.set_content(Some(&stack));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    // Files given on the command line or through "Open with" go to the editor.
    let import = gio::SimpleAction::new("import-path", Some(glib::VariantTy::STRING));
    {
        let a = app.clone();
        let stack = stack.clone();
        import.connect_activate(move |_, v| {
            if let Some(path) = v.and_then(|v| v.get::<String>()) {
                stack.set_visible_child_name("editor");
                a.import_file(path.into());
            }
        });
    }
    window.add_action(&import);

    {
        let a = app.clone();
        app.connect(move |c| {
            if c == Change::Busy {
                match a.busy() {
                    Some(b) => {
                        banner.set_title(&b);
                        banner.set_revealed(true);
                    }
                    None => banner.set_revealed(false),
                }
                while let Some(m) = a.messages.borrow_mut().pop_front() {
                    toasts.add_toast(adw::Toast::builder().title(m).timeout(4).build());
                }
            }
            if c == Change::Appearance {
                let s = a.settings.borrow();
                style::apply(s.color_scheme, s.aperture_accent);
            }
        });
    }
    {
        let a = app.clone();
        window.connect_close_request(move |_| {
            a.stop_live();
            a.player.stop();
            a.save_now();
            gtk4::glib::Propagation::Proceed
        });
    }
    window.present();
    window.upcast()
}

/// Opens an audio file in the editor of a window made by [`build`].
pub fn open(window: &gtk4::Window, path: &Path) {
    let _ = window.activate_action("win.import-path", Some(&path.to_string_lossy().to_variant()));
}

fn appearance_menu(app: &Rc<App>) -> gtk4::MenuButton {
    let b = gtk4::Box::new(Orientation::Vertical, 10);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b.set_margin_top(12);
    b.set_margin_bottom(12);
    b.append(&gtk4::Label::builder().label("Appearance").xalign(0.0).css_classes(["heading"]).build());
    let modes = gtk4::Box::new(Orientation::Horizontal, 0);
    modes.add_css_class("linked");
    let current = app.settings.borrow().color_scheme;
    let mut first: Option<gtk4::ToggleButton> = None;
    for (scheme, label) in [(ColorScheme::System, "System"), (ColorScheme::Light, "Light"), (ColorScheme::Dark, "Dark")] {
        let t = gtk4::ToggleButton::with_label(label);
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        t.set_active(scheme == current);
        let a = app.clone();
        t.connect_toggled(move |t| {
            if t.is_active() {
                let accent = a.settings.borrow().aperture_accent;
                a.set_color_scheme(scheme, accent);
            }
        });
        modes.append(&t);
    }
    b.append(&modes);
    let accent_row = gtk4::Box::new(Orientation::Horizontal, 12);
    accent_row.append(&gtk4::Label::builder().label("Aperture orange accent").xalign(0.0).hexpand(true).build());
    let accent = gtk4::Switch::builder().active(app.settings.borrow().aperture_accent).valign(gtk4::Align::Center).build();
    {
        let a = app.clone();
        accent.connect_active_notify(move |s| {
            let scheme = a.settings.borrow().color_scheme;
            a.set_color_scheme(scheme, s.is_active());
        });
    }
    accent_row.append(&accent);
    b.append(&accent_row);
    b.append(
        &gtk4::Label::builder()
            .label("Off follows your desktop's accent colour.")
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build(),
    );
    let popover = gtk4::Popover::builder().child(&b).build();
    gtk4::MenuButton::builder().icon_name("applications-graphics-symbolic").tooltip_text("Appearance").popover(&popover).build()
}
