//! Unit tests for the settings model.

use super::*;

#[test]
fn chords_parse_in_any_order_and_round_trip() {
    let chord = Chord::parse("Shift+Ctrl+Z");
    assert!(chord.ctrl && chord.shift && !chord.alt);
    assert_eq!(chord.key, Some("Z"));
    assert_eq!(chord.display(), "Ctrl+Shift+Z");
    assert_eq!(Chord::parse("F9").display(), "F9");
    assert_eq!(Chord::parse("ctrl+space").display(), "Ctrl+Space");
}

#[test]
fn binding_an_action_replaces_the_previous_chord() {
    let mut settings = Settings::default();
    settings.bind(ShortcutAction::Undo, "Ctrl+U");
    assert_eq!(settings.keys_for(ShortcutAction::Undo), "Ctrl+U");
    let count = settings
        .shortcuts
        .iter()
        .filter(|binding| binding.action == ShortcutAction::Undo)
        .count();
    assert_eq!(count, 1);
}

#[test]
fn binding_an_unlisted_action_creates_it() {
    let mut settings = Settings::default();
    settings.unbind(ShortcutAction::WindowSamples);
    assert!(settings.keys_for(ShortcutAction::WindowSamples).is_empty());
    settings.bind(ShortcutAction::WindowSamples, "F12");
    assert_eq!(settings.keys_for(ShortcutAction::WindowSamples), "F12");
}

#[test]
fn conflicts_are_detectable() {
    let mut settings = Settings::default();
    settings.bind(ShortcutAction::Undo, "Ctrl+K");
    assert_eq!(
        settings.actions_for_chord("Ctrl+K"),
        vec![ShortcutAction::Undo]
    );
    assert!(settings.actions_for_chord("Ctrl+Shift+Q").is_empty());
}

#[test]
fn older_settings_gain_new_commands_but_keep_unbinds() {
    let mut settings = Settings::default();
    // Simulate a file written before the debug panel existed.
    settings
        .shortcuts
        .retain(|binding| binding.action != ShortcutAction::WindowDebug);
    settings.unbind(ShortcutAction::WindowSamples);
    settings.normalize();
    assert_eq!(
        settings.keys_for(ShortcutAction::WindowDebug),
        "Ctrl+Shift+D"
    );
    // A deliberate unbind keeps its (empty) entry and stays unbound.
    assert!(settings.keys_for(ShortcutAction::WindowSamples).is_empty());
}

#[test]
fn plugin_roots_join_every_format_list() {
    let mut settings = Settings::default();
    settings.vst_paths.push("C:/vst".to_owned());
    settings.vst3_paths.push("C:/vst3".to_owned());
    settings.clap_paths.push(String::new());
    settings.clap_paths.push("C:/clap".to_owned());
    assert_eq!(
        settings.plugin_roots(),
        vec!["C:/vst", "C:/vst3", "C:/clap"]
    );
}

#[test]
fn settings_round_trip_through_json() {
    let mut settings = Settings {
        parameter_smoothing: false,
        smoothing_ms: 30.0,
        ..Settings::default()
    };
    settings.bind(ShortcutAction::ToolPen, "Ctrl+1");
    let json = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert!(!restored.parameter_smoothing);
    assert!((restored.smoothing_ms - 30.0).abs() < f32::EPSILON);
    assert_eq!(restored.keys_for(ShortcutAction::ToolPen), "Ctrl+1");
}
