use eframe::egui::Ui;

/// What a power switch was asked to do, decided from the modifier keys at click
/// time. Living here lets context actions carry the decision instead of
/// re-reading the keyboard later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerIntent {
    pub toggle: bool,
    pub lock: bool,
    pub solo: bool,
}

impl PowerIntent {
    pub fn any(self) -> bool {
        self.toggle || self.lock || self.solo
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextAction {
    ToggleLanePin(usize),
    DisconnectChannel(usize),
    SelectChannel(usize),
    DeleteClip {
        lane: usize,
        clip: usize,
    },
    DuplicateClip {
        lane: usize,
        clip: usize,
    },
    MakeClipPatternUnique {
        lane: usize,
        clip: usize,
    },
    /// Opens the rename prompt for a track and its mixer channel.
    RenameTrack(usize),
    /// Moves a track one row up or down.
    MoveTrack {
        lane: usize,
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
    /// Removes an instrument from the project.
    RemoveInstrument(usize),
    /// Sets or clears an instrument's custom colour.
    SetInstrumentColor {
        instrument: usize,
        color: Option<[u8; 3]>,
    },
    /// Opens the plug-in picker for an instrument.
    OpenInstrumentPicker(usize),
    /// Toggles one insert slot's power switch.
    ToggleInsertEnabled {
        channel: usize,
        slot: usize,
        intent: PowerIntent,
    },
    /// Opens the plug-in picker for one insert slot.
    OpenInsertPicker {
        channel: usize,
        slot: usize,
    },
    /// Opens the plug-in window for one insert slot.
    OpenInsertWindow {
        channel: usize,
        slot: usize,
    },
    /// Enables or disables a whole mixer channel.
    ToggleChannelEnabled(usize),
    /// Clears one insert slot.
    ClearInsert {
        channel: usize,
        slot: usize,
    },
}

#[derive(Clone, Debug)]
pub enum MenuItem {
    Action {
        label: String,
        action: ContextAction,
    },
    Separator,
    Folder {
        label: String,
        items: Vec<MenuItem>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct ContextMenu {
    pub items: Vec<MenuItem>,
}

impl ContextMenu {
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
                        action: ContextAction::DisconnectChannel(2),
                    }],
                },
            ],
        };
        assert_eq!(menu.items.len(), 3);
    }
}
