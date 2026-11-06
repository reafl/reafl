//! One mixer channel strip.

use eframe::egui;
use pulseforge_core::project::ParameterTarget;

use crate::context_menu::{ContextAction, ContextMenu, MenuItem};
use crate::fader::fader;
use crate::icons::{Icon, paint_icon};
use crate::knob::Knob;
use crate::meter::render_meter_sized;
use crate::power::PowerSwitch;
use crate::theme::lane_color;
use crate::tooltip::tooltip;

use pulseforge_core::project;

use pulseforge_core::project::{InsertFxSlot, ThreeBandEq};

use crate::eq_curve::draw_eq_curve;
use crate::fader_math::fader_label;
use crate::power::power_switch;

/// What one mixer strip is bound to.
pub struct MixerStrip<'a> {
    /// Index into `project.lanes`, or the master index.
    pub channel_id: usize,
    /// Channel name, edited in place when present.
    pub name: Option<&'a mut String>,
    /// Whether the channel row is pinned.
    pub pinned: Option<&'a mut bool>,
    /// Linear fader gain.
    pub gain: &'a mut f32,
    /// Pan, `-1` left to `1` right.
    pub pan: &'a mut f32,
    /// Stereo width; absent on the master strip.
    pub width: Option<&'a mut f32>,
    /// The channel's three-band EQ gains.
    pub eq: &'a mut ThreeBandEq,
    /// Peak level for the meter, `0..=1`.
    pub meter: f32,
    /// Whether this strip is the selected channel.
    pub selected: bool,
    /// Draws the compact form when true.
    pub collapsed: bool,
    /// True for the master strip.
    pub is_master: bool,
    /// Whether the channel is switched on.
    pub enabled: bool,
    /// Whether the channel ignores edits.
    pub locked: bool,
    /// Whether the channel is soloed.
    pub solo: bool,
    /// Insert slots; absent when the strip hides them.
    pub inserts: Option<&'a mut [InsertFxSlot]>,
    /// Channel FX bypass, absent on the master strip.
    pub fx_bypass: Option<&'a mut bool>,
    /// Insert slots whose plug-in window is currently open.
    pub open_inserts: &'a std::collections::BTreeSet<(usize, usize)>,
}

#[derive(Default)]
/// What one mixer strip is bound to.
pub struct MixerStripResult {
    /// True when any control on the strip changed.
    pub changed: bool,
    /// True when the strip asked to become the selected channel.
    pub selected: bool,
    /// Menu actions the view still has to apply.
    pub actions: Vec<ContextAction>,
}

/// Meter heights shared by the fader and the level meter so the pair reads as
/// one control.
/// Height of the fader and meter pair.
pub const MIXER_FADER_HEIGHT: f32 = 132.0;
/// Width of the fader.
pub const MIXER_FADER_WIDTH: f32 = 30.0;

/// Draws one mixer channel strip.
///
/// # Parameters
///
/// - `ui`: the parent layout; the strip allocates its own column.
/// - `strip`: everything the strip reads and writes.
///
/// # Returns
///
/// What changed, whether the strip was selected, and any menu actions.
pub fn mixer_strip(ui: &mut egui::Ui, mut strip: MixerStrip<'_>) -> MixerStripResult {
    let mut result = MixerStripResult::default();
    let channel_color = if strip.is_master {
        egui::Color32::WHITE
    } else {
        lane_color(strip.channel_id)
    };
    let strip_width = if strip.collapsed { 96.0 } else { 206.0 };
    // A non-zero height keeps every strip in the horizontal row aligned at the
    // top instead of drifting down as each one grows.
    let strip_height = ui.available_height().max(120.0);
    // Any part of the strip selects the channel. The hit test uses the rect
    // recorded last frame so it can be registered *before* the widgets it
    // contains; a later registration would swallow their clicks.
    let bg_rect_id = egui::Id::new(("mixer_strip_rect", strip.channel_id));
    if let Some(rect) = ui.data(|data| data.get_temp::<egui::Rect>(bg_rect_id)) {
        let background = ui.interact(
            rect,
            egui::Id::new(("mixer_strip_hit", strip.channel_id)),
            egui::Sense::click(),
        );
        if background.clicked() {
            result.selected = true;
        }
        if background.secondary_clicked() {
            result.selected = true;
        }
    }
    // The strip owns a fixed width; allocating it up front stops the parent
    // horizontal layout from stretching it across the whole scroll area.
    let frame = ui
        .allocate_ui_with_layout(
            egui::vec2(strip_width, strip_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::group(ui.style())
                    .fill(egui::Color32::from_gray(29))
                    .stroke(egui::Stroke::new(
                        if strip.selected { 2.0_f32 } else { 1.0_f32 },
                        if strip.selected {
                            egui::Color32::WHITE
                        } else {
                            channel_color
                        },
                    ))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_min_width(strip_width - 16.0);
                        ui.set_max_width(strip_width - 16.0);

                        // Header: one power switch (shift locks, ctrl solos) plus the name,
                        // which is also the channel's selection target.
                        ui.horizontal(|ui| {
                            let power_size = egui::vec2(18.0, 18.0);
                            let (power_rect, power_response) =
                                ui.allocate_exact_size(power_size, egui::Sense::click());
                            let painter = ui.painter_at(power_rect);
                            painter.rect_filled(
                                power_rect,
                                3.0,
                                if strip.enabled {
                                    channel_color.gamma_multiply(0.45)
                                } else {
                                    egui::Color32::from_gray(38)
                                },
                            );
                            paint_icon(
                                &painter,
                                power_rect.shrink(2.5),
                                Icon::Power,
                                if strip.enabled {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_gray(110)
                                },
                            );
                            if strip.locked {
                                painter.line_segment(
                                    [
                                        egui::pos2(
                                            power_rect.left() + 2.0,
                                            power_rect.bottom() - 2.0,
                                        ),
                                        egui::pos2(
                                            power_rect.right() - 2.0,
                                            power_rect.bottom() - 2.0,
                                        ),
                                    ],
                                    egui::Stroke::new(
                                        2.0_f32,
                                        egui::Color32::from_rgb(230, 178, 92),
                                    ),
                                );
                            }
                            if strip.solo {
                                painter.circle_filled(
                                    egui::pos2(power_rect.right() - 3.0, power_rect.top() + 3.0),
                                    2.5,
                                    egui::Color32::from_rgb(120, 200, 255),
                                );
                            }
                            if power_response.clicked() {
                                result
                                    .actions
                                    .push(ContextAction::ToggleChannelEnabled(strip.channel_id));
                            }
                            tooltip(ui, &power_response, "Power - shift locks, ctrl solos");

                            let mut heading = if strip.is_master {
                                "MASTER".to_owned()
                            } else {
                                strip.name.as_deref().map_or_else(
                                    || format!("Insert {}", strip.channel_id + 1),
                                    |name| name.clone(),
                                )
                            };
                            if strip.collapsed && heading.chars().count() > 9 {
                                heading =
                                    format!("{}?", heading.chars().take(8).collect::<String>());
                            }
                            // The editable name already shows the channel, so
                            // no separate label is drawn beside it.
                            let editable =
                                !strip.is_master && strip.name.is_some() && !strip.collapsed;
                            let header = if editable {
                                ui.label(egui::RichText::new("").small())
                            } else {
                                let response = ui.selectable_label(
                                    strip.selected,
                                    egui::RichText::new(heading.clone())
                                        .color(channel_color)
                                        .strong(),
                                );
                                result.selected |= response.clicked();
                                response
                            };
                            if !strip.is_master {
                                let menu = ContextMenu {
                                    items: vec![
                                        MenuItem::Action {
                                            label: if strip
                                                .pinned
                                                .as_deref()
                                                .copied()
                                                .unwrap_or(false)
                                            {
                                                "Unpin channel"
                                            } else {
                                                "Pin channel"
                                            }
                                            .to_owned(),
                                            action: ContextAction::ToggleLanePin(strip.channel_id),
                                        },
                                        MenuItem::Action {
                                            label: if strip.enabled {
                                                "Disable channel"
                                            } else {
                                                "Enable channel"
                                            }
                                            .to_owned(),
                                            action: ContextAction::ToggleChannelEnabled(
                                                strip.channel_id,
                                            ),
                                        },
                                        MenuItem::Separator,
                                        MenuItem::Folder {
                                            label: "Routing".to_owned(),
                                            items: vec![MenuItem::Action {
                                                label: "Disconnect all cables".to_owned(),
                                                action: ContextAction::DisconnectChannel(
                                                    strip.channel_id,
                                                ),
                                            }],
                                        },
                                        MenuItem::Folder {
                                            label: "Channel".to_owned(),
                                            items: vec![MenuItem::Action {
                                                label: "Select channel".to_owned(),
                                                action: ContextAction::SelectChannel(
                                                    strip.channel_id,
                                                ),
                                            }],
                                        },
                                    ],
                                };
                                header.context_menu(|ui| {
                                    if let Some(action) = menu.show(ui) {
                                        result.actions.push(action);
                                    }
                                });
                                if let Some(name) = &mut strip.name
                                    && !strip.collapsed
                                    && ui
                                        .add(
                                            egui::TextEdit::singleline(&mut **name)
                                                .desired_width((strip_width - 60.0).max(60.0))
                                                .margin(egui::Margin::symmetric(4, 1)),
                                        )
                                        .changed()
                                {
                                    result.changed = true;
                                }
                            }
                        });

                        // Fader and meter: one centred pair with identical height.
                        let fader_height = if strip.collapsed {
                            66.0
                        } else {
                            MIXER_FADER_HEIGHT
                        };
                        let fader_width = if strip.collapsed {
                            20.0
                        } else {
                            MIXER_FADER_WIDTH
                        };
                        let pair_width = fader_width + 6.0 + 13.0;
                        let pad = ((strip_width - pair_width) * 0.5).max(0.0);
                        let gain_target = ParameterTarget::MixerChannel {
                            channel_id: strip.channel_id,
                            parameter: project::MixerParameter::Gain,
                        };
                        let pan_target = ParameterTarget::MixerChannel {
                            channel_id: strip.channel_id,
                            parameter: project::MixerParameter::Pan,
                        };
                        let width_target = ParameterTarget::MixerChannel {
                            channel_id: strip.channel_id,
                            parameter: project::MixerParameter::Width,
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(pad);
                            if fader(
                                ui,
                                strip.gain,
                                fader_width,
                                fader_height,
                                Some(gain_target),
                            ) {
                                result.changed = true;
                            }
                            render_meter_sized(ui, strip.meter, 13.0, fader_height);
                        });
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(fader_label(*strip.gain)).small());
                            ui.horizontal(|ui| {
                                result.changed |= Knob::new("", -1.0, 1.0, "")
                                    .compact()
                                    .automates(pan_target)
                                    .show(ui, strip.pan);
                                if let Some(width) = strip.width.as_deref_mut() {
                                    result.changed |= Knob::new("", 0.0, 2.0, "")
                                        .with_default(1.0)
                                        .compact()
                                        .automates(width_target)
                                        .show(ui, width);
                                }
                            });
                        });

                        if !strip.collapsed
                            && let Some(inserts) = strip.inserts.as_deref_mut()
                        {
                            ui.separator();
                            if let Some(bypass) = strip.fx_bypass.as_deref_mut() {
                                // Dry/wet comparison that never touches the
                                // individual insert enable states.
                                if ui.selectable_label(*bypass, "FX bypass").clicked() {
                                    *bypass = !*bypass;
                                    result.changed = true;
                                }
                            }
                            let any_solo = inserts.iter().any(|slot| slot.solo && slot.enabled);
                            let rows: Vec<(usize, String, bool, bool, bool)> = inserts
                                .iter()
                                .enumerate()
                                .map(|(index, slot)| {
                                    (
                                        index,
                                        if slot.name.is_empty() {
                                            format!("Slot {}", index + 1)
                                        } else {
                                            slot.name.clone()
                                        },
                                        slot.enabled,
                                        slot.solo,
                                        slot.locked,
                                    )
                                })
                                .collect();
                            // Ten slots can be taller than the strip, so the
                            // list scrolls and every insert stays reachable.
                            egui::ScrollArea::vertical()
                                .id_salt(("mixer_inserts", strip.channel_id))
                                .max_height(150.0)
                                // A fixed height keeps every strip the same size
                                // whether or not it carries inserts.
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                            for (index, label, enabled, solo, locked) in rows {
                                let open = strip.open_inserts.contains(&(strip.channel_id, index));
                                ui.horizontal(|ui| {
                                    let intent = power_switch(
                                        ui,
                                        PowerSwitch {
                                            name: &label,
                                            enabled,
                                            locked,
                                            solo,
                                            any_solo,
                                            color: channel_color,
                                            size: egui::vec2(16.0, 16.0),
                                        },
                                    );
                                    if intent.any() {
                                        result.actions.push(ContextAction::ToggleInsertEnabled {
                                            channel: strip.channel_id,
                                            slot: index,
                                            intent,
                                        });
                                    }

                                    let text =
                                        egui::RichText::new(label).small().color(if enabled {
                                            egui::Color32::from_gray(225)
                                        } else {
                                            egui::Color32::from_gray(130)
                                        });
                                    let slot_button = ui.add_sized(
                                        [ui.available_width().max(60.0), 16.0],
                                        egui::Button::new(text)
                                            .fill(if open {
                                                egui::Color32::from_gray(62)
                                            } else {
                                                egui::Color32::from_gray(38)
                                            })
                                            .sense(egui::Sense::click()),
                                    );
                                    if slot_button.clicked() {
                                        result.actions.push(ContextAction::OpenInsertWindow {
                                            channel: strip.channel_id,
                                            slot: index,
                                        });
                                    }
                                    if slot_button.double_clicked() {
                                        result.actions.push(ContextAction::OpenInsertPicker {
                                            channel: strip.channel_id,
                                            slot: index,
                                        });
                                    }
                                    tooltip(
                                        ui,
                                        &slot_button,
                                        "Click: open - double click or middle click: change effect",
                                    );
                                    slot_button.context_menu(|ui| {
                                        let menu = ContextMenu {
                                            items: vec![
                                                MenuItem::Action {
                                                    label: "Change effect".to_owned(),
                                                    action: ContextAction::OpenInsertPicker {
                                                        channel: strip.channel_id,
                                                        slot: index,
                                                    },
                                                },
                                                MenuItem::Action {
                                                    label: "Open window".to_owned(),
                                                    action: ContextAction::OpenInsertWindow {
                                                        channel: strip.channel_id,
                                                        slot: index,
                                                    },
                                                },
                                                MenuItem::Separator,
                                                MenuItem::Action {
                                                    label: "Clear insert".to_owned(),
                                                    action: ContextAction::ClearInsert {
                                                        channel: strip.channel_id,
                                                        slot: index,
                                                    },
                                                },
                                            ],
                                        };
                                        if let Some(action) = menu.show(ui) {
                                            result.actions.push(action);
                                        }
                                    });
                                });
                            }
                                });
                            ui.label(
                                egui::RichText::new("Middle click changes an insert")
                                    .small()
                                    .color(egui::Color32::from_gray(120)),
                            );
                        }

                        if !strip.collapsed {
                            ui.separator();
                            ui.label(egui::RichText::new("3-BAND EQ").strong());
                            draw_eq_curve(ui, *strip.eq);
                            ui.horizontal(|ui| {
                                result.changed |= Knob::new("LOW", -12.0, 12.0, " dB")
                                    .show(ui, &mut strip.eq.low_db);
                                result.changed |= Knob::new("MID", -12.0, 12.0, " dB")
                                    .show(ui, &mut strip.eq.mid_db);
                                result.changed |= Knob::new("HIGH", -12.0, 12.0, " dB")
                                    .show(ui, &mut strip.eq.high_db);
                            });
                        }
                        if strip.pinned.as_deref().copied().unwrap_or(false) {
                            ui.label(egui::RichText::new("PINNED").small().color(channel_color));
                        }
                    });
            },
        )
        .response;
    ui.data_mut(|data| data.insert_temp(bg_rect_id, frame.rect));
    result
}

