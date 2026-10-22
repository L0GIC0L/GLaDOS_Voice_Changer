//! Controls generated from the settings schema, Simple-mode sliders, and the preset library.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use glados_dsp::model::{Macro, PresetCategory, ParamSpec, P, PRESETS};
use glados_dsp::scales::{self, NOTE_NAMES};
use gtk4::{Align, Orientation};

use crate::app::{App, Change};

fn dim_label(text: &str) -> gtk4::Label {
    let l = gtk4::Label::builder().label(text).wrap(true).xalign(0.0).css_classes(["dim-label", "caption"]).build();
    l.set_natural_wrap_mode(gtk4::NaturalWrapMode::Word);
    l
}

/// A list row holding any widget, padded like the other rows.
pub fn plain_row(child: &impl IsA<gtk4::Widget>) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::builder().activatable(false).selectable(false).build();
    child.set_margin_start(12);
    child.set_margin_end(12);
    child.set_margin_top(8);
    child.set_margin_bottom(8);
    row.set_child(Some(child));
    row
}

/// A titled slider with its value shown on the right; double-click the title to reset.
fn slider_box(
    title: &str, help: &str, min: f64, max: f64, step: f64,
) -> (gtk4::Box, gtk4::Scale, gtk4::Label, gtk4::Box) {
    let outer = gtk4::Box::new(Orientation::Vertical, 2);
    let header = gtk4::Box::new(Orientation::Horizontal, 8);
    let label = gtk4::Label::builder().label(title).xalign(0.0).hexpand(true).build();
    let value = gtk4::Label::builder().xalign(1.0).css_classes(["accent", "numeric"]).build();
    header.append(&label);
    header.append(&value);
    let scale = gtk4::Scale::with_range(Orientation::Horizontal, min, max, if step > 0.0 { step } else { (max - min) / 1000.0 });
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    outer.append(&header);
    outer.append(&scale);
    if !help.is_empty() {
        outer.append(&dim_label(help));
    }
    (outer, scale, value, header)
}

fn help_text(s: &ParamSpec) -> String {
    if s.live_only && !s.help.starts_with("Live only") { format!("Live only. {}", s.help) } else { s.help.to_string() }
}

/// The right control for a setting: switch, drop-down or slider, kept in step with the app's settings.
pub fn param_control(app: &Rc<App>, p: P) -> gtk4::Widget {
    let s = p.spec();
    let help = help_text(s);
    if s.toggle {
        let row = adw::SwitchRow::builder().title(s.label).build();
        if !help.is_empty() {
            row.set_subtitle(&help);
        }
        row.set_active(app.params().bool(p));
        let a = app.clone();
        row.connect_active_notify(move |r| {
            let v = if r.is_active() { 1.0 } else { 0.0 };
            if a.params().get(p) != v {
                a.set_param(p, v);
            }
        });
        let weak = row.downgrade();
        let a = app.clone();
        app.connect(move |c| {
            if c == Change::Params {
                if let Some(r) = weak.upgrade() {
                    let v = a.params().bool(p);
                    if r.is_active() != v {
                        r.set_active(v);
                    }
                }
            }
        });
        return row.upcast();
    }
    if let Some(choices) = s.choices {
        let model = gtk4::StringList::new(choices);
        let row = adw::ComboRow::builder().title(s.label).model(&model).build();
        if !help.is_empty() {
            row.set_subtitle(&help);
        }
        row.set_selected(app.params().int(p).max(0) as u32);
        let a = app.clone();
        row.connect_selected_notify(move |r| {
            let v = r.selected() as f32;
            if a.params().get(p) != v {
                a.set_param(p, v);
            }
        });
        let weak = row.downgrade();
        let a = app.clone();
        app.connect(move |c| {
            if c == Change::Params {
                if let Some(r) = weak.upgrade() {
                    let v = a.params().int(p).max(0) as u32;
                    if r.selected() != v {
                        r.set_selected(v);
                    }
                }
            }
        });
        return row.upcast();
    }
    let (outer, scale, value, header) = slider_box(s.label, &help, s.min as f64, s.max as f64, s.step as f64);
    let busy = Rc::new(Cell::new(false));
    scale.set_value(app.params().get(p) as f64);
    value.set_label(&s.format(app.params().get(p)));
    {
        let a = app.clone();
        let busy = busy.clone();
        let value = value.clone();
        scale.connect_value_changed(move |sc| {
            if busy.get() {
                return;
            }
            let v = s.clamp(sc.value() as f32);
            value.set_label(&s.format(v));
            if a.params().get(p) != v {
                a.set_param(p, v);
            }
        });
    }
    // Double-click the name to put it back to its default.
    let click = gtk4::GestureClick::new();
    {
        let a = app.clone();
        click.connect_pressed(move |_, n, _, _| {
            if n == 2 {
                a.set_param(p, s.default);
            }
        });
    }
    header.add_controller(click);
    let weak_scale = scale.downgrade();
    let weak_value = value.downgrade();
    let a = app.clone();
    app.connect(move |c| {
        if c == Change::Params {
            if let (Some(sc), Some(vl)) = (weak_scale.upgrade(), weak_value.upgrade()) {
                let v = a.params().get(p);
                if s.clamp(sc.value() as f32) != v {
                    busy.set(true);
                    sc.set_value(v as f64);
                    busy.set(false);
                }
                vl.set_label(&s.format(v));
            }
        }
    });
    plain_row(&outer).upcast()
}

/// A Simple-mode slider that moves several settings at once.
pub fn macro_control(app: &Rc<App>, m: &'static Macro) -> gtk4::Widget {
    let (outer, scale, value, _) = slider_box(m.label, m.help, 0.0, 100.0, 1.0);
    let busy = Rc::new(Cell::new(false));
    let show = |v: f32| format!("{:.0} %", v.clamp(0.0, 100.0));
    let current = m.read(&app.params()).clamp(0.0, 100.0);
    scale.set_value(current as f64);
    value.set_label(&show(current));
    scale.set_widget_name(&format!("macro-{}", m.id));
    {
        let a = app.clone();
        let busy = busy.clone();
        scale.connect_value_changed(move |sc| {
            if !busy.get() {
                a.set_macro(m, sc.value() as f32);
            }
        });
    }
    let weak_scale = scale.downgrade();
    let weak_value = value.downgrade();
    let a = app.clone();
    app.connect(move |c| {
        if c == Change::Params {
            if let (Some(sc), Some(vl)) = (weak_scale.upgrade(), weak_value.upgrade()) {
                let v = m.read(&a.params()).clamp(0.0, 100.0);
                if (sc.value() as f32 - v).abs() > 0.5 {
                    busy.set(true);
                    sc.set_value(v as f64);
                    busy.set(false);
                }
                vl.set_label(&show(v));
            }
        }
    });
    plain_row(&outer).upcast()
}

/// Twelve note buttons for building a custom scale; only shown while the scale is Custom.
pub fn custom_scale_row(app: &Rc<App>) -> gtk4::Widget {
    let grid = gtk4::Grid::builder().row_spacing(4).column_spacing(4).column_homogeneous(true).build();
    let buttons: Rc<Vec<gtk4::ToggleButton>> = Rc::new(
        (0..12)
            .map(|i| {
                let b = gtk4::ToggleButton::builder().label(NOTE_NAMES[i]).css_classes(["note-chip"]).build();
                grid.attach(&b, (i % 6) as i32, (i / 6) as i32, 1, 1);
                b
            })
            .collect(),
    );
    let busy = Rc::new(Cell::new(false));
    let sync = {
        let buttons = buttons.clone();
        let busy = busy.clone();
        move |mask: i32| {
            busy.set(true);
            for (i, b) in buttons.iter().enumerate() {
                b.set_active((mask >> i) & 1 == 1);
            }
            busy.set(false);
        }
    };
    sync(app.params().int(P::CustomMask));
    for (i, b) in buttons.iter().enumerate() {
        let a = app.clone();
        let busy = busy.clone();
        b.connect_toggled(move |_| {
            if !busy.get() {
                let mask = a.params().int(P::CustomMask) ^ (1 << i);
                a.set_param(P::CustomMask, mask as f32);
            }
        });
    }
    let row = plain_row(&grid);
    row.set_visible(app.params().int(P::Scale) == scales::CUSTOM);
    let weak = row.downgrade();
    let a = app.clone();
    app.connect(move |c| {
        if c == Change::Params {
            if let Some(r) = weak.upgrade() {
                r.set_visible(a.params().int(P::Scale) == scales::CUSTOM);
                sync(a.params().int(P::CustomMask));
            }
        }
    });
    row.upcast()
}

/// A row naming the current preset. Activating it opens the whole library.
pub fn preset_card(app: &Rc<App>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title("Preset").activatable(true).build();
    row.add_css_class("property");
    row.add_suffix(&gtk4::Image::from_icon_name("view-grid-symbolic"));
    let update = {
        let row = row.downgrade();
        let a = app.clone();
        move || {
            let Some(row) = row.upgrade() else { return };
            match a.preset_name() {
                Some(name) => {
                    let blurb = PRESETS.iter().find(|p| p.name == name).map(|p| p.blurb).unwrap_or("One of your saved presets.");
                    row.set_subtitle(&format!("{name}\n{blurb}"));
                }
                None => row.set_subtitle(&format!("Custom settings\nChoose from {} voices.", PRESETS.len())),
            }
        }
    };
    update();
    app.connect(move |c| {
        if matches!(c, Change::Params | Change::Presets) {
            update();
        }
    });
    let a = app.clone();
    row.connect_activated(move |r| presets_dialog(&a, r));
    row
}

/// The preset library: every built-in preset by category, plus saved ones. Tapping applies at once, so presets
/// can be auditioned while the clip plays.
pub fn presets_dialog(app: &Rc<App>, parent: &impl IsA<gtk4::Widget>) {
    let dialog = adw::Dialog::builder().title("Presets").content_width(560).content_height(640).build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let play = gtk4::Button::from_icon_name("media-playback-start-symbolic");
    play.set_tooltip_text(Some("Play or pause the clip"));
    {
        let a = app.clone();
        play.connect_clicked(move |_| a.toggle_play());
    }
    play.set_sensitive(app.clip().is_some());
    header.pack_start(&play);
    let save = gtk4::Button::with_label("Save current...");
    header.pack_end(&save);
    toolbar.add_top_bar(&header);

    let content = gtk4::Box::new(Orientation::Vertical, 12);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.set_margin_top(12);
    content.set_margin_bottom(18);
    let blurb = dim_label("Tap a preset to hear it. Your clip keeps playing.");
    content.append(&blurb);

    let chips: Rc<std::cell::RefCell<Vec<(String, gtk4::ToggleButton)>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let busy = Rc::new(Cell::new(false));
    let refresh: Rc<dyn Fn()> = {
        let chips = chips.clone();
        let busy = busy.clone();
        let a = app.clone();
        let blurb = blurb.clone();
        Rc::new(move || {
            let current = a.preset_name();
            busy.set(true);
            for (name, b) in chips.borrow().iter() {
                b.set_active(current.as_deref() == Some(name.as_str()));
            }
            busy.set(false);
            if let Some(p) = current.as_deref().and_then(|n| PRESETS.iter().find(|p| p.name == n)) {
                blurb.set_label(p.blurb);
            }
        })
    };

    let flow = |names: Vec<String>| {
        let fb = gtk4::FlowBox::builder().selection_mode(gtk4::SelectionMode::None).column_spacing(6).row_spacing(6).max_children_per_line(6).build();
        for name in names {
            let b = gtk4::ToggleButton::builder().label(name.as_str()).css_classes(["preset-chip"]).build();
            let a = app.clone();
            let n = name.clone();
            let busy = busy.clone();
            let refresh = refresh.clone();
            b.connect_toggled(move |_| {
                if !busy.get() {
                    a.apply_preset(&n);
                    refresh();
                }
            });
            chips.borrow_mut().push((name, b.clone()));
            fb.append(&b);
        }
        fb
    };
    for cat in PresetCategory::ALL {
        content.append(&gtk4::Label::builder().label(cat.title()).xalign(0.0).css_classes(["heading"]).build());
        content.append(&flow(PRESETS.iter().filter(|p| p.category == cat).map(|p| p.name.to_string()).collect()));
    }
    content.append(&gtk4::Label::builder().label("Your presets").xalign(0.0).css_classes(["heading"]).build());
    let users = app.user_preset_names();
    if users.is_empty() {
        content.append(&dim_label("None yet. Adjust a voice, then use Save current."));
    } else {
        content.append(&flow(users.clone()));
        let del = gtk4::Box::new(Orientation::Horizontal, 6);
        let names = gtk4::StringList::new(&users.iter().map(String::as_str).collect::<Vec<_>>());
        let pick = gtk4::DropDown::new(Some(names), gtk4::Expression::NONE);
        let remove = gtk4::Button::with_label("Delete");
        remove.add_css_class("destructive-action");
        {
            let a = app.clone();
            let pick = pick.clone();
            let users = users.clone();
            let dialog = dialog.clone();
            remove.connect_clicked(move |_| {
                if let Some(n) = users.get(pick.selected() as usize) {
                    a.delete_user_preset(n);
                    dialog.close();
                }
            });
        }
        del.append(&pick);
        del.append(&remove);
        content.append(&del);
    }
    refresh();

    {
        let a = app.clone();
        let dialog_weak = dialog.downgrade();
        save.connect_clicked(move |b| {
            let entry = gtk4::Entry::builder().placeholder_text("Name").max_length(32).activates_default(true).build();
            let ask = adw::AlertDialog::new(Some("Save preset"), Some("Saves every voice setting under a name of your choice."));
            ask.set_extra_child(Some(&entry));
            ask.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
            ask.set_response_appearance("save", adw::ResponseAppearance::Suggested);
            ask.set_default_response(Some("save"));
            let a = a.clone();
            let dialog_weak = dialog_weak.clone();
            ask.connect_response(None, move |_, resp| {
                if resp == "save" && a.save_user_preset(&entry.text()) {
                    if let Some(d) = dialog_weak.upgrade() {
                        d.close();
                    }
                }
            });
            ask.present(Some(b));
        });
    }

    let scroller = gtk4::ScrolledWindow::builder().hscrollbar_policy(gtk4::PolicyType::Never).vexpand(true).child(&content).build();
    toolbar.set_content(Some(&scroller));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

/// A small horizontal level meter.
pub fn level_bar() -> gtk4::LevelBar {
    let bar = gtk4::LevelBar::builder().min_value(0.0).max_value(1.0).hexpand(true).valign(Align::Center).build();
    bar.add_css_class("meter");
    bar.set_mode(gtk4::LevelBarMode::Continuous);
    // Drop the default low/high colour offsets: a plain accent bar reads best.
    for name in ["low", "high", "full"] {
        bar.remove_offset_value(Some(name));
    }
    bar
}

pub fn page_box() -> gtk4::Box {
    let b = gtk4::Box::new(Orientation::Vertical, 18);
    b.set_margin_start(18);
    b.set_margin_end(18);
    b.set_margin_top(18);
    b.set_margin_bottom(24);
    b
}

/// Wraps a page in a scroller with a comfortable maximum width.
pub fn scrolled_page(child: &impl IsA<gtk4::Widget>) -> gtk4::ScrolledWindow {
    let clamp = adw::Clamp::builder().maximum_size(820).child(child).build();
    gtk4::ScrolledWindow::builder().hscrollbar_policy(gtk4::PolicyType::Never).vexpand(true).child(&clamp).build()
}
