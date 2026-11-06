//! The power switch and its modifier intents.

use eframe::egui;

use crate::context_menu::PowerIntent;
use crate::icons::{Icon, paint_icon};
use crate::tooltip::tooltip;

/// What a power switch was asked to do, decided from the modifier keys at click

/// One power switch: what it is called, what it is doing, and how big to draw it.
pub struct PowerSwitch<'a> {
    /// Name used by the tooltip.
    pub name: &'a str,
    /// Whether the row is switched on.
    pub enabled: bool,
    /// Whether the row is locked against edits.
    pub locked: bool,
    /// Whether the row is soloed.
    pub solo: bool,
    /// True when another switch of the same group is soloed (dims the badge).
    pub any_solo: bool,
    /// Accent colour of the switch.
    pub color: egui::Color32,
    /// Size of the switch in points.
    pub size: egui::Vec2,
}

/// Paint and hit-test a power switch inside `rect`.
pub fn power_switch_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    switch: &PowerSwitch<'_>,
) -> PowerIntent {
    let response = ui.interact(rect, id, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        3.0,
        if switch.enabled {
            switch.color.gamma_multiply(0.45)
        } else {
            egui::Color32::from_gray(38)
        },
    );
    paint_icon(
        &painter,
        rect.shrink(switch.size.x * 0.15),
        Icon::Power,
        if switch.enabled {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(110)
        },
    );
    if switch.locked {
        painter.line_segment(
            [
                egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
            ],
            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(230, 178, 92)),
        );
    }
    if switch.solo || switch.any_solo {
        painter.circle_filled(
            egui::pos2(rect.right() - 3.0, rect.top() + 3.0),
            2.5,
            if switch.solo {
                egui::Color32::from_rgb(120, 200, 255)
            } else {
                egui::Color32::from_gray(70)
            },
        );
    }
    tooltip(
        ui,
        &response,
        &format!("{} · click powers, shift locks, ctrl solos", switch.name),
    );
    if !response.clicked() {
        return PowerIntent::default();
    }
    ui.input(|input| {
        if input.modifiers.shift {
            PowerIntent {
                lock: true,
                ..PowerIntent::default()
            }
        } else if input.modifiers.command || input.modifiers.ctrl {
            PowerIntent {
                solo: true,
                ..PowerIntent::default()
            }
        } else {
            PowerIntent {
                toggle: true,
                ..PowerIntent::default()
            }
        }
    })
}

/// Power switch laid out by the caller's `ui`, returning the requested intent.
pub fn power_switch(ui: &mut egui::Ui, switch: PowerSwitch<'_>) -> PowerIntent {
    let (rect, _) = ui.allocate_exact_size(switch.size, egui::Sense::hover());
    let id = ui.id().with(("power_switch", switch.name, rect.left() as i32, rect.top() as i32));
    power_switch_at(ui, rect, id, &switch)
}

