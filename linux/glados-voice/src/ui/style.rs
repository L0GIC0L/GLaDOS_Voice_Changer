//! Colours for the custom-drawn views, and the Aperture accent.

use gtk4::prelude::*;
use gtk4::{cairo, gdk, pango};

use crate::settings::ColorScheme;

/// Draws `text` with its baseline at (x, y), in the widget's font at `px` pixels. Pango rather than cairo's own text
/// calls, so glyphs the main font lacks (such as ♯) come from another font instead of showing as boxes.
pub fn draw_text(cr: &cairo::Context, widget: &impl IsA<gtk4::Widget>, text: &str, px: f64, x: f64, y: f64) {
    let layout = widget.create_pango_layout(Some(text));
    let mut font = widget.pango_context().font_description().unwrap_or_default();
    font.set_absolute_size(px * pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    cr.move_to(x, y - layout.baseline() as f64 / pango::SCALE as f64);
    pangocairo::functions::show_layout(cr, &layout);
}

#[derive(Clone, Copy)]
pub struct Rgba(pub f64, pub f64, pub f64, pub f64);

impl Rgba {
    pub fn with_alpha(self, a: f64) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }

    pub fn set(self, cr: &gtk4::cairo::Context) {
        cr.set_source_rgba(self.0, self.1, self.2, self.3);
    }
}

/// The palette the piano roll and pitch trace draw with, following light or dark mode and the accent.
pub struct Palette {
    pub background: Rgba,
    pub panel: Rgba,
    pub text: Rgba,
    pub dim: Rgba,
    pub line: Rgba,
    pub accent: Rgba,
    pub note: Rgba,
    pub note_edge: Rgba,
    pub handle: Rgba,
    pub playhead: Rgba,
}

const APERTURE_ORANGE: Rgba = Rgba(1.0, 0.604, 0.0, 1.0);
const ADWAITA_BLUE: Rgba = Rgba(0.208, 0.518, 0.894, 1.0);

pub fn palette(aperture_accent: bool) -> Palette {
    let dark = adw::StyleManager::default().is_dark();
    let accent = if aperture_accent { APERTURE_ORANGE } else { ADWAITA_BLUE };
    if dark {
        Palette {
            background: Rgba(0.12, 0.12, 0.13, 1.0),
            panel: Rgba(0.17, 0.17, 0.19, 1.0),
            text: Rgba(0.92, 0.92, 0.92, 1.0),
            dim: Rgba(0.65, 0.65, 0.68, 1.0),
            line: Rgba(1.0, 1.0, 1.0, 0.12),
            accent,
            note: Rgba(0.42, 0.45, 0.62, 1.0),
            note_edge: Rgba(0.70, 0.74, 0.95, 1.0),
            handle: Rgba(0.55, 0.85, 0.75, 1.0),
            playhead: Rgba(0.95, 0.35, 0.35, 1.0),
        }
    } else {
        Palette {
            background: Rgba(0.97, 0.97, 0.98, 1.0),
            panel: Rgba(0.91, 0.91, 0.93, 1.0),
            text: Rgba(0.12, 0.12, 0.14, 1.0),
            dim: Rgba(0.40, 0.40, 0.45, 1.0),
            line: Rgba(0.0, 0.0, 0.0, 0.12),
            accent,
            note: Rgba(0.78, 0.80, 0.93, 1.0),
            note_edge: Rgba(0.33, 0.36, 0.62, 1.0),
            handle: Rgba(0.10, 0.55, 0.45, 1.0),
            playhead: Rgba(0.85, 0.15, 0.15, 1.0),
        }
    }
}

/// Applies light, dark or system mode and, if chosen, the Aperture orange accent.
pub fn apply(scheme: ColorScheme, aperture_accent: bool) {
    let sm = adw::StyleManager::default();
    sm.set_color_scheme(match scheme {
        ColorScheme::System => adw::ColorScheme::Default,
        ColorScheme::Light => adw::ColorScheme::ForceLight,
        ColorScheme::Dark => adw::ColorScheme::ForceDark,
    });
    thread_local! {
        static PROVIDER: gtk4::CssProvider = gtk4::CssProvider::new();
        static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    PROVIDER.with(|p| {
        let accent = if aperture_accent {
            "@define-color accent_bg_color #ff9a00;\n@define-color accent_fg_color #1a0f00;\n@define-color accent_color #e08600;\n"
        } else {
            ""
        };
        p.load_from_string(&format!(
            "{accent}
            .meter trough {{ min-height: 6px; }}
            .meter block.filled {{ background-color: @accent_bg_color; }}
            .big-button {{ padding: 10px 24px; font-size: 1.15em; }}
            .preset-chip {{ border-radius: 999px; padding: 4px 12px; }}
            .note-chip {{ min-width: 34px; padding: 4px 2px; }}"
        ));
        if !INSTALLED.replace(true) {
            if let Some(display) = gdk::Display::default() {
                gtk4::style_context_add_provider_for_display(&display, p, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
            }
        }
    });
}
