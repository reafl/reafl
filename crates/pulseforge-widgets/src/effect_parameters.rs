//! Controls for one insert slot's parameters.

use eframe::egui;
use pulseforge_core::project::{EffectParameter, InsertFxSlot, ParameterTarget};

use crate::knob::Knob;

use pulseforge_core::project;

/// Draws the parameter knobs for one insert slot.
///
/// # Parameters
///
/// - `ui`: the parent layout.
/// - `slot`: the insert slot whose parameters are edited.
/// - `track_id`: mixer channel owning the slot.
/// - `slot_id`: insert slot index.
///
/// # Returns
///
/// `true` when any parameter changed.
pub fn render_effect_parameters(
    ui: &mut egui::Ui,
    slot: &mut InsertFxSlot,
    track_id: usize,
    slot_id: usize,
) -> bool {
    let mut changed = false;
    if slot.enabled && !slot.name.is_empty() {
        ui.horizontal(|ui| {
            changed |= Knob::new("MIX", 0.0, 1.0, "")
                .with_default(0.5)
                .automates(ParameterTarget::InsertEffect {
                    track_id,
                    slot_id,
                    parameter: EffectParameter::Mix,
                })
                .show(ui, &mut slot.mix);
            match slot.name.as_str() {
                "3-Band EQ" => {
                    changed |= Knob::new("LOW", -12.0, 12.0, " dB")
                        .with_default(0.0)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::LowDb,
                        })
                        .show(ui, &mut slot.eq.low_db);
                    changed |= Knob::new("MID", -12.0, 12.0, " dB")
                        .with_default(0.0)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::MidDb,
                        })
                        .show(ui, &mut slot.eq.mid_db);
                    changed |= Knob::new("HIGH", -12.0, 12.0, " dB")
                        .with_default(0.0)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::HighDb,
                        })
                        .show(ui, &mut slot.eq.high_db);
                }
                "Reverb" | "Plate Reverb" => {
                    changed |= Knob::new("DECAY", 0.0, 1.0, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::Amount,
                        })
                        .show(ui, &mut slot.amount);
                    changed |= Knob::new("ROOM", 0.0, 1.0, "")
                        .with_default(0.55)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::RoomSize,
                        })
                        .show(ui, &mut slot.room_size);
                }
                "Delay" | "Tape Echo" => {
                    changed |= Knob::new("WET", 0.0, 1.0, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::Amount,
                        })
                        .show(ui, &mut slot.amount);
                    egui::ComboBox::from_id_salt(("effect_time_unit", track_id, slot_id))
                        .selected_text(match slot.time_unit {
                            project::TimeUnit::Steps => "Steps",
                            project::TimeUnit::Milliseconds => "ms",
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    &mut slot.time_unit,
                                    project::TimeUnit::Steps,
                                    "Steps",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut slot.time_unit,
                                    project::TimeUnit::Milliseconds,
                                    "Milliseconds",
                                )
                                .changed();
                        });
                    let mut time_amount = slot.time_amount();
                    match slot.time_unit {
                        project::TimeUnit::Steps => {
                            changed |= Knob::new("TIME", 0.01, 16.0, " steps")
                                .with_default(1.0)
                                .automates(ParameterTarget::InsertEffect {
                                    track_id,
                                    slot_id,
                                    parameter: EffectParameter::TimeSeconds,
                                })
                                .show(ui, &mut time_amount);
                        }
                        project::TimeUnit::Milliseconds => {
                            changed |= Knob::new("TIME", 10.0, 2_000.0, " ms")
                                .with_default(320.0)
                                .automates(ParameterTarget::InsertEffect {
                                    track_id,
                                    slot_id,
                                    parameter: EffectParameter::TimeSeconds,
                                })
                                .show(ui, &mut time_amount);
                        }
                    }
                    slot.set_time_amount(time_amount);
                    changed |= Knob::new("FEEDBACK", 0.0, 0.94, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::Feedback,
                        })
                        .show(ui, &mut slot.feedback);
                }
                "Juno Chorus" | "Phaser" => {
                    changed |= Knob::new("AMOUNT", 0.0, 1.0, "")
                        .with_default(0.35)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::Amount,
                        })
                        .show(ui, &mut slot.amount);
                    changed |= Knob::new("RATE", 0.05, 8.0, " Hz")
                        .with_default(0.8)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::RateHz,
                        })
                        .show(ui, &mut slot.rate_hz);
                    changed |= Knob::new("DEPTH", 0.0, 1.0, "")
                        .with_default(0.5)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::Depth,
                        })
                        .show(ui, &mut slot.depth);
                }
                "Waveshaper" => {
                    changed |= Knob::new("DRIVE", 0.0, 1.0, "")
                        .with_default(0.5)
                        .automates(ParameterTarget::InsertEffect {
                            track_id,
                            slot_id,
                            parameter: EffectParameter::Drive,
                        })
                        .show(ui, &mut slot.drive);
                }
                _ => {}
            }
        });
    }
    changed
}
