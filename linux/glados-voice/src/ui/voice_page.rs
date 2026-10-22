//! Presets on top, then a few combined sliders (Simple) or every setting, grouped (Advanced).

use std::rc::Rc;

use adw::prelude::*;
use glados_dsp::model::{Group, MACROS, P};
use gtk4::Orientation;

use super::widgets::{custom_scale_row, macro_control, page_box, param_control, preset_card, scrolled_page};
use crate::app::{App, Change};

pub fn build(app: &Rc<App>) -> gtk4::Widget {
    let page = page_box();

    let presets = adw::PreferencesGroup::new();
    presets.add(&preset_card(app));
    page.append(&presets);

    let mode = gtk4::Box::new(Orientation::Horizontal, 0);
    mode.add_css_class("linked");
    mode.set_halign(gtk4::Align::Center);
    let simple = gtk4::ToggleButton::with_label("Simple");
    let advanced = gtk4::ToggleButton::with_label("Advanced");
    advanced.set_group(Some(&simple));
    simple.set_widget_name("mode-simple");
    advanced.set_widget_name("mode-advanced");
    mode.append(&simple);
    mode.append(&advanced);
    page.append(&mode);

    let stack = gtk4::Stack::builder().transition_type(gtk4::StackTransitionType::Crossfade).vhomogeneous(false).build();
    stack.add_named(&simple_page(app), Some("simple"));
    stack.add_named(&advanced_page(app), Some("advanced"));
    page.append(&stack);

    let show = {
        let stack = stack.clone();
        let simple = simple.clone();
        let advanced = advanced.clone();
        let a = app.clone();
        move || {
            let on = a.settings.borrow().simple_mode;
            stack.set_visible_child_name(if on { "simple" } else { "advanced" });
            if simple.is_active() != on {
                simple.set_active(on);
            }
            if advanced.is_active() == on {
                advanced.set_active(!on);
            }
        }
    };
    show();
    {
        let a = app.clone();
        simple.connect_toggled(move |b| {
            if b.is_active() && !a.settings.borrow().simple_mode {
                a.set_simple_mode(true);
            }
        });
    }
    {
        let a = app.clone();
        advanced.connect_toggled(move |b| {
            if b.is_active() && a.settings.borrow().simple_mode {
                a.set_simple_mode(false);
            }
        });
    }
    app.connect(move |c| {
        if c == Change::Appearance {
            show();
        }
    });
    scrolled_page(&page).upcast()
}

fn simple_page(app: &Rc<App>) -> gtk4::Widget {
    let group = adw::PreferencesGroup::builder()
        .title("Quick controls")
        .description("Pick a preset, then season it here. Most sliders move several settings at once; Advanced shows them all. Presets never change Noise reduction.")
        .build();
    for p in [P::Denoise, P::Shift, P::Formant, P::Tempo, P::SpeedRandom, P::HarmonyMix] {
        group.add(&param_control(app, p));
    }
    for m in MACROS.iter() {
        group.add(&macro_control(app, m));
    }
    group.add(&param_control(app, P::OutDb));
    group.upcast()
}

fn advanced_page(app: &Rc<App>) -> gtk4::Widget {
    let b = gtk4::Box::new(Orientation::Vertical, 12);
    b.append(
        &gtk4::Label::builder()
            .label("Double-click a slider's name to reset it. Settings are saved automatically.")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .build(),
    );
    let list = gtk4::ListBox::builder().selection_mode(gtk4::SelectionMode::None).css_classes(["boxed-list"]).build();
    for g in Group::ALL {
        let row = adw::ExpanderRow::builder().title(g.title()).subtitle(g.blurb()).expanded(g == Group::Tuning).build();
        for p in P::in_group(g) {
            row.add_row(&param_control(app, p));
            if p == P::Scale {
                row.add_row(&custom_scale_row(app));
            }
        }
        let reset = gtk4::Button::builder().label(format!("Reset {}", g.title().to_lowercase())).halign(gtk4::Align::Start).css_classes(["flat"]).build();
        {
            let a = app.clone();
            reset.connect_clicked(move |_| {
                let ps: Vec<P> = P::in_group(g).collect();
                a.reset_params(&ps);
            });
        }
        row.add_row(&super::widgets::plain_row(&reset));
        list.append(&row);
    }
    b.append(&list);
    b.upcast()
}
