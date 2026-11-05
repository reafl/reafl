//! Mixer chain presets stored on disk. 


use pulseforge_core::project::InsertFxSlot;

/// Folder that holds saved mixer-chain presets, one JSON file per preset.
pub const PRESET_DIR: &str = "presets/mixer";

/// Turns a user-typed preset name into a file stem that is safe on every
/// platform: letters, digits, spaces, dashes and underscores survive, everything
/// else becomes an underscore.
pub fn preset_file_stem(name: &str) -> String {
    let mut stem: String = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    stem = stem.trim().to_owned();
    if stem.is_empty() {
        stem = "Preset".to_owned();
    }
    stem
}

/// Writes `inserts` to `presets/mixer/<name>.json` and returns the preset name.
pub fn save_chain_preset(name: &str, inserts: &[InsertFxSlot]) -> Result<String, String> {
    let stem = preset_file_stem(name);
    std::fs::create_dir_all(PRESET_DIR)
        .map_err(|error| format!("Could not create {PRESET_DIR}: {error}"))?;
    let json = serde_json::to_string_pretty(inserts)
        .map_err(|error| format!("Could not encode preset: {error}"))?;
    let path = format!("{PRESET_DIR}/{stem}.json");
    std::fs::write(&path, json).map_err(|error| format!("Could not write {path}: {error}"))?;
    Ok(stem)
}

/// Every saved preset name, sorted alphabetically.
pub fn list_chain_presets() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(PRESET_DIR) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("json"))
                .then(|| path.file_stem()?.to_str().map(ToOwned::to_owned))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

/// Reads one preset back. Missing slots are simply not applied, so an older
/// preset still fits a channel with more inserts.
pub fn load_chain_preset(name: &str) -> Result<Vec<InsertFxSlot>, String> {
    let path = format!("{PRESET_DIR}/{}.json", preset_file_stem(name));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {path}: {error}"))?;
    serde_json::from_str::<Vec<InsertFxSlot>>(&text)
        .map_err(|error| format!("Invalid preset {path}: {error}"))
}

