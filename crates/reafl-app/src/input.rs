//! Keyboard bindings: chord lookup and the command dispatcher.

use crate::*;

impl ReaflApp {
    /// Maps a chord key name to its egui key.
    pub(crate) fn egui_key(name: &str) -> Option<egui::Key> {
        use egui::Key;
        Some(match name {
            "Space" => Key::Space,
            "Enter" => Key::Enter,
            "Tab" => Key::Tab,
            "Escape" => Key::Escape,
            "Delete" => Key::Delete,
            "Home" => Key::Home,
            "Comma" => Key::Comma,
            "Period" => Key::Period,
            "Minus" => Key::Minus,
            "F2" => Key::F2,
            "F5" => Key::F5,
            "F6" => Key::F6,
            "F7" => Key::F7,
            "F8" => Key::F8,
            "F9" => Key::F9,
            "F10" => Key::F10,
            "F11" => Key::F11,
            "F12" => Key::F12,
            "A" => Key::A,
            "B" => Key::B,
            "C" => Key::C,
            "D" => Key::D,
            "E" => Key::E,
            "F" => Key::F,
            "G" => Key::G,
            "H" => Key::H,
            "I" => Key::I,
            "J" => Key::J,
            "K" => Key::K,
            "L" => Key::L,
            "M" => Key::M,
            "N" => Key::N,
            "O" => Key::O,
            "P" => Key::P,
            "Q" => Key::Q,
            "R" => Key::R,
            "S" => Key::S,
            "T" => Key::T,
            "U" => Key::U,
            "V" => Key::V,
            "W" => Key::W,
            "X" => Key::X,
            "Y" => Key::Y,
            "Z" => Key::Z,
            "0" => Key::Num0,
            "1" => Key::Num1,
            "2" => Key::Num2,
            "3" => Key::Num3,
            "4" => Key::Num4,
            "5" => Key::Num5,
            "6" => Key::Num6,
            "7" => Key::Num7,
            "8" => Key::Num8,
            "9" => Key::Num9,
            _ => return None,
        })
    }

    /// Turns the event stream into actions using the configured key map, so
    /// editing a binding in Settings immediately changes behaviour.
    pub(crate) fn pressed_actions(&self, context: &egui::Context) -> Vec<ShortcutAction> {
        let mut pressed = Vec::new();
        for binding in &self.settings.shortcuts {
            if !binding.enabled || binding.keys.trim().is_empty() {
                continue;
            }
            let chord = Chord::parse(&binding.keys);
            let Some(key) = chord.key.and_then(Self::egui_key) else {
                continue;
            };
            let matched = context.input(|input| {
                input.key_pressed(key)
                    && input.modifiers.ctrl == chord.ctrl
                    && input.modifiers.shift == chord.shift
                    && input.modifiers.alt == chord.alt
            });
            if matched {
                pressed.push(binding.action);
            }
        }
        pressed
    }

    /// Executes one command. Every shortcut and toolbar button routes here, so
    /// behaviour stays identical however the command is triggered.
    pub(crate) fn perform(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::TransportToggle => {
                self.playing = !self.playing;
                self.send_audio(AudioCommand::SetPlaying(self.playing));
            }
            ShortcutAction::TransportStop => {
                self.playing = false;
                // Rewind first so the engine's position is already reset when
                // the next play command arrives.
                self.send_audio(AudioCommand::Rewind);
                self.send_audio(AudioCommand::SetPlaying(false));
                self.playhead_display = 0.0;
                self.playhead_raw = 0;
            }
            ShortcutAction::ToggleLoop => {
                if self.arrangement_loop.is_some() {
                    self.loop_enabled = !self.loop_enabled;
                } else {
                    self.status = "Drag above the arrangement to create a loop first.".to_owned();
                }
            }
            ShortcutAction::TogglePatternPlayback => {
                self.selected_clip_playback = !self.selected_clip_playback;
            }
            ShortcutAction::ToggleMetronome => {
                self.metronome = !self.metronome;
                self.send_audio(AudioCommand::SetMetronome(self.metronome));
            }
            ShortcutAction::WindowChannelRack => {
                self.windows.channel_rack = !self.windows.channel_rack;
            }
            ShortcutAction::WindowPianoRoll => {
                self.windows.piano_roll = !self.windows.piano_roll;
            }
            ShortcutAction::WindowMixer => self.windows.mixer = !self.windows.mixer,
            ShortcutAction::WindowRouting => self.windows.routing = !self.windows.routing,
            ShortcutAction::WindowAutomation => self.windows.automation = !self.windows.automation,
            ShortcutAction::WindowMidi => self.windows.midi_routing = !self.windows.midi_routing,
            ShortcutAction::WindowSamples => self.windows.samples = !self.windows.samples,
            ShortcutAction::WindowSettings => self.windows.settings = !self.windows.settings,
            ShortcutAction::WindowRender => self.windows.render = !self.windows.render,
            ShortcutAction::WindowDebug => self.debug_panel.toggle(),
            ShortcutAction::ToolPen => self.set_tool(PianoTool::Pen),
            ShortcutAction::ToolSelect => self.set_tool(PianoTool::Select),
            ShortcutAction::ToolBrush => self.set_tool(PianoTool::Brush),
            ShortcutAction::ToolJoin => self.set_tool(PianoTool::Join),
            ShortcutAction::ToolCut => self.set_tool(PianoTool::Cut),
            ShortcutAction::SnapUp => self.step_snap(1),
            ShortcutAction::SnapDown => self.step_snap(-1),
            ShortcutAction::ControlVelocity => self.control_lane = ControlLane::Velocity,
            ShortcutAction::ControlRepeats => self.control_lane = ControlLane::Repeats,
            ShortcutAction::NoteUp => self.shift_selected_notes(1, 0, false),
            ShortcutAction::NoteDown => self.shift_selected_notes(-1, 0, false),
            ShortcutAction::NoteOctaveUp => self.shift_selected_notes(12, 0, false),
            ShortcutAction::NoteOctaveDown => self.shift_selected_notes(-12, 0, false),
            ShortcutAction::NoteLeft => self.shift_selected_notes(0, -1, true),
            ShortcutAction::NoteRight => self.shift_selected_notes(0, 1, true),
            ShortcutAction::Undo => self.undo(),
            ShortcutAction::Redo => self.redo(),
            ShortcutAction::SelectAll => self.select_all_notes(),
            ShortcutAction::DuplicateClip => self.duplicate_selected_clip(),
            ShortcutAction::DeleteSelection => self.delete_selected_clip(),
            ShortcutAction::RenameTrack => self.begin_rename_track(),
            ShortcutAction::RouteToFreeInsert => {
                // Tracks are their own mixer channels now, so the shortcut
                // wraps the selected track in a folder instead of routing it to
                // a free insert.
                let track = self.selected_instrument;
                if self.project.group_track(track).is_some() {
                    self.sync_project();
                    self.status = "Grouped the selected track.".to_owned();
                }
            }
            ShortcutAction::ClearSelection => self.selection = None,
            ShortcutAction::ResetPan => {
                self.arrangement_pan_steps = 0;
                self.arrangement_pan_lanes = 0;
                self.pattern_pan_steps = 0;
                self.piano_pan_steps = 0;
            }
            ShortcutAction::SaveProject => self.save_project(),
            ShortcutAction::LoadProject => self.load_project(),
        }
    }
}
