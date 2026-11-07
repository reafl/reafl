//! Right-click menus modelled as data.
//!
//! A window describes what a menu offers and returns a [`ContextAction`]; it
//! never draws buttons itself. That keeps every menu in the editor looking the
//! same, and it lets a view build its menu from state without owning any of the
//! behaviour behind it.

use eframe::egui::Ui;

/// What a power switch was asked to do, decided from the modifier keys at click
/// time. Living here lets context actions carry the decision instead of
/// re-reading the keyboard later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerIntent {
    /// A plain click: flip the switch.
    pub toggle: bool,
    /// Shift-click: lock the row so edits are ignored.
    pub lock: bool,
    /// Ctrl-click: solo the row, muting its siblings.
    pub solo: bool,
}

impl PowerIntent {
    /// True when any of the three intents was requested.
    ///
    /// # Returns
    ///
    /// `true` for a toggle, lock or solo click.
    pub fn any(self) -> bool {
        self.toggle || self.lock || self.solo
    }
}

/// Everything a right-click menu can ask the editor to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextAction {
    /// Pins or unpins an arrangement lane.
    ToggleLanePin(usize),
    /// Removes every send from a channel.
    DisconnectChannel(usize),
    /// Makes a mixer channel the selected one.
    SelectChannel(usize),
    /// Deletes a clip from a lane.
    DeleteClip {
        /// Arrangement lane holding the clip.
        lane: usize,
        /// Index of the clip inside that lane.
        clip: usize,
    },
    /// Copies a clip in place after itself.
    DuplicateClip {
        /// Arrangement lane holding the clip.
        lane: usize,
        /// Index of the clip inside that lane.
        clip: usize,
    },
    /// Gives a clip its own pattern so edits stop affecting its siblings.
    MakeClipPatternUnique {
        /// Arrangement lane holding the clip.
        lane: usize,
        /// Index of the clip inside that lane.
        clip: usize,
    },
    /// Opens the rename prompt for a track and its mixer channel.
    RenameTrack(usize),
    /// Moves a track one row up or down.
    MoveTrack {
        /// Arrangement lane to move.
        lane: usize,
        /// `true` to move up, `false` to move down.
        up: bool,
    },
    /// Shows or hides a track's automation rows.
    ToggleTrackAutomation(usize),
    /// Deletes an automation lane.
    DeleteAutomation(usize),
    /// Duplicates an automation lane so one parameter can hold several clips.
    DuplicateAutomation(usize),
    /// Adds an empty automation lane grouped under a track.
    NewAutomationForTrack(usize),
    /// Opens the settings window on the plug-in page.
    OpenPluginSettings,
    /// Starts an inline rename in the channel rack.
    RenameInstrument(usize),
    /// Wraps a track in a new folder.
    GroupTrack(usize),
    /// Removes an instrument from the project.
    RemoveInstrument(usize),
    /// Sets or clears an instrument's custom colour.
    SetInstrumentColor {
        /// Instrument to recolour.
        instrument: usize,
        /// RGB values, or `None` to fall back to the lane colour.
        color: Option<[u8; 3]>,
    },
    /// Opens the plug-in picker for an instrument.
    OpenInstrumentPicker(usize),
    /// Toggles one insert slot's power switch.
    ToggleInsertEnabled {
        /// Mixer channel owning the slot.
        channel: usize,
        /// Insert slot index.
        slot: usize,
        /// What the click asked for.
        intent: PowerIntent,
    },
    /// Opens the plug-in picker for one insert slot.
    OpenInsertPicker {
        /// Mixer channel owning the slot.
        channel: usize,
        /// Insert slot index.
        slot: usize,
    },
    /// Opens the plug-in window for one insert slot.
    OpenInsertWindow {
        /// Mixer channel owning the slot.
        channel: usize,
        /// Insert slot index.
        slot: usize,
    },
    /// Enables or disables a whole mixer channel.
    ToggleChannelEnabled(usize),
    /// Clears one insert slot.
    ClearInsert {
        /// Mixer channel owning the slot.
        channel: usize,
        /// Insert slot index.
        slot: usize,
    },
}

/// One entry of a [`ContextMenu`].
#[derive(Clone, Debug)]
pub enum MenuItem {
    /// A clickable entry carrying the action it stands for.
    Action {
        /// Text shown in the menu.
        label: String,
        /// Action returned when the entry is clicked.
        action: ContextAction,
    },
    /// A horizontal rule between groups of entries.
    Separator,
    /// A submenu holding further entries.
    Folder {
        /// Text shown for the submenu.
        label: String,
        /// Entries inside the submenu.
        items: Vec<MenuItem>,
    },
}

/// A whole menu, drawn as one unit.
#[derive(Clone, Debug, Default)]
pub struct ContextMenu {
    /// Entries in display order.
    pub items: Vec<MenuItem>,
}

impl ContextMenu {
    /// Draws the menu and reports what was picked.
    ///
    /// # Parameters
    ///
    /// - `ui`: the menu's own layout, normally the body of
    ///   `egui::Response::context_menu`.
    ///
    /// # Returns
    ///
    /// The picked action, or `None` while nothing has been chosen. Submenus are
    /// resolved recursively, so a nested pick is returned from the outer call.
    pub fn show(&self, ui: &mut Ui) -> Option<ContextAction> {
        for item in &self.items {
            match item {
                MenuItem::Action { label, action } => {
                    if ui.button(label).clicked() {
                        ui.close();
                        return Some(action.clone());
                    }
                }
                MenuItem::Separator => {
                    ui.separator();
                }
                MenuItem::Folder { label, items } => {
                    let mut selected = None;
                    ui.menu_button(label, |ui| {
                        selected = ContextMenu {
                            items: items.clone(),
                        }
                        .show(ui)
                    });
                    if selected.is_some() {
                        return selected;
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_supports_actions_separators_and_nested_folders() {
        let menu = ContextMenu {
            items: vec![
                MenuItem::Action {
                    label: "Pin".to_owned(),
                    action: ContextAction::ToggleLanePin(2),
                },
                MenuItem::Separator,
                MenuItem::Folder {
                    label: "Route".to_owned(),
                    items: vec![MenuItem::Action {
                        label: "Disconnect".to_owned(),
                        action: ContextAction::DisconnectChannel(3),
                    }],
                },
            ],
        };
        assert_eq!(menu.items.len(), 3);
        assert!(!PowerIntent::default().any());
        assert!(
            PowerIntent {
                solo: true,
                ..PowerIntent::default()
            }
            .any()
        );
    }
}
