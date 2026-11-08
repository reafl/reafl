//! External plug-in discovery and hosting boundaries.
//!
//! Discovery is deliberately separate from processing: scanning only reads
//! file-system metadata, so it can run on the UI thread while the audio thread
//! keeps a plain list of resolved plug-in references. Processing goes through
//! [`PluginHost`], which keeps the real-time path free of any format-specific
//! code and lets a later build swap in a real VST3/CLAP host without touching
//! the mixer.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Plug-in formats reafl can discover.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginFormat {
    /// Classic VST 2.x bundle.
    Vst2,
    /// VST 3 bundle, a directory on every supported platform.
    Vst3,
    /// CLAP bundle.
    Clap,
}

impl PluginFormat {
    /// Human readable label for menus and tooltips.
    pub fn label(self) -> &'static str {
        match self {
            Self::Vst2 => "VST",
            Self::Vst3 => "VST3",
            Self::Clap => "CLAP",
        }
    }

    /// File extensions this format uses on the current platform.
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Vst2 => &["dll", "so", "vst", "dylib"],
            Self::Vst3 => &["vst3"],
            Self::Clap => &["clap"],
        }
    }

    /// True when `path` looks like a bundle for this format.
    fn matches_path(self, path: &Path) -> bool {
        if self == Self::Vst3 {
            // VST3 ships as a directory bundle on every supported platform.
            return path.is_dir()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("vst3"));
        }
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return false;
        };
        path.is_file()
            && self
                .extensions()
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    }
}

/// A discovered external plug-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalPlugin {
    /// Format the bundle was discovered as.
    pub format: PluginFormat,
    /// Display name, normally the file stem with separators cleaned up.
    pub name: String,
    /// Absolute path of the bundle.
    pub path: String,
}

impl ExternalPlugin {
    /// Label shown in the plug-in picker, e.g. `VST3 · Surge XT`.
    pub fn label(&self) -> String {
        format!("{} · {}", self.format.label(), self.name)
    }

    fn from_path(format: PluginFormat, path: &Path) -> Self {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Plugin");
        Self {
            format,
            name: clean_name(stem),
            path: path.to_string_lossy().into_owned(),
        }
    }
}

/// Turns `Some_Plugin-x64` into `Some Plugin` for display.
fn clean_name(stem: &str) -> String {
    let mut name = stem.replace(['_', '-'], " ");
    for suffix in [" x64", " vst3", " 64", " 64bit"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_owned();
        }
    }
    name.trim().to_owned()
}

/// Directories scanned when the user has not configured any locations.
pub fn default_plugin_directories() -> Vec<(PluginFormat, PathBuf)> {
    let mut directories = Vec::new();
    #[cfg(windows)]
    {
        let program_files = std::env::var_os("ProgramFiles")
            .map_or_else(|| PathBuf::from("C:/Program Files"), PathBuf::from);
        let common = std::env::var_os("CommonProgramFiles").map_or_else(
            || PathBuf::from("C:/Program Files/Common Files"),
            PathBuf::from,
        );
        directories.push((PluginFormat::Vst2, program_files.join("VSTPlugins")));
        directories.push((PluginFormat::Vst2, common.join("VST2")));
        directories.push((PluginFormat::Vst3, common.join("VST3")));
        directories.push((PluginFormat::Clap, common.join("CLAP")));
    }
    #[cfg(target_os = "macos")]
    {
        directories.push((
            PluginFormat::Vst2,
            PathBuf::from("/Library/Audio/Plug-Ins/VST"),
        ));
        directories.push((
            PluginFormat::Vst3,
            PathBuf::from("/Library/Audio/Plug-Ins/VST3"),
        ));
        directories.push((
            PluginFormat::Clap,
            PathBuf::from("/Library/Audio/Plug-Ins/CLAP"),
        ));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // System locations, then the per-user ones every Linux plug-in
        // installer writes to (`~/.vst3`, `~/.clap`).
        directories.push((PluginFormat::Vst2, PathBuf::from("/usr/lib/vst")));
        directories.push((PluginFormat::Vst2, PathBuf::from("/usr/local/lib/vst")));
        directories.push((PluginFormat::Vst3, PathBuf::from("/usr/lib/vst3")));
        directories.push((PluginFormat::Vst3, PathBuf::from("/usr/local/lib/vst3")));
        directories.push((PluginFormat::Clap, PathBuf::from("/usr/lib/clap")));
        directories.push((PluginFormat::Clap, PathBuf::from("/usr/local/lib/clap")));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            directories.push((PluginFormat::Vst2, home.join(".vst")));
            directories.push((PluginFormat::Vst3, home.join(".vst3")));
            directories.push((PluginFormat::Clap, home.join(".clap")));
        }
    }
    directories
}

/// Scans the supplied roots (and the platform defaults for any format the user
/// left empty) one directory level deep, which is where plugin installers place
/// their bundles.
pub fn scan_plugins(roots: &[String], max_per_root: usize) -> Vec<ExternalPlugin> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let configured: Vec<(PluginFormat, PathBuf)> = roots
        .iter()
        .filter(|root| !root.trim().is_empty())
        .map(|root| (detect_format(Path::new(root)), PathBuf::from(root)))
        .collect();
    let mut search = configured;
    for (format, directory) in default_plugin_directories() {
        if !search.iter().any(|(_, existing)| *existing == directory) {
            search.push((format, directory));
        }
    }

    for (_format, directory) in search {
        if results.len() >= max_per_root {
            break;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut found = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let detected = [PluginFormat::Vst3, PluginFormat::Clap, PluginFormat::Vst2]
                .into_iter()
                .find(|candidate| candidate.matches_path(&path));
            let Some(detected) = detected else {
                continue;
            };
            let plugin = ExternalPlugin::from_path(detected, &path);
            if seen.insert(plugin.path.clone()) {
                results.push(plugin);
                found += 1;
                if found >= max_per_root || results.len() >= max_per_root {
                    break;
                }
            }
        }
    }
    results.sort_by(|left, right| left.name.cmp(&right.name));
    results
}

/// Guesses the format a user-supplied root path is intended for.
fn detect_format(path: &Path) -> PluginFormat {
    let text = path.to_string_lossy().to_ascii_lowercase();
    if text.contains("clap") {
        PluginFormat::Clap
    } else if text.contains("vst3") {
        PluginFormat::Vst3
    } else {
        PluginFormat::Vst2
    }
}

/// Identity read from the bundle itself rather than guessed from its file name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Name the bundle reports for itself.
    pub name: String,
    /// Vendor the bundle reports for itself.
    pub vendor: String,
    /// Version string the bundle reports.
    pub version: String,
    /// Class names declared by a VST3 module, in file order.
    pub classes: Vec<String>,
    /// Sub-categories declared by a VST3 module (for example "Fx", "Instrument").
    pub categories: Vec<String>,
}

impl PluginMetadata {
    /// True when the bundle itself published metadata, as opposed to the name
    /// simply being derived from the file name.
    pub fn is_informative(&self) -> bool {
        !(self.vendor.is_empty() && self.version.is_empty() && self.classes.is_empty())
    }
}

/// Reads whatever identity the bundle publishes.
///
/// VST3 bundles ship a `Contents/moduleinfo.json` describing the module, so that
/// file is parsed. Other formats have no portable metadata inside the bundle, so
/// only the file name is available.
pub fn read_metadata(path: &str) -> PluginMetadata {
    let root = Path::new(path);
    let mut metadata = PluginMetadata {
        name: root
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map_or_else(String::new, clean_name),
        ..PluginMetadata::default()
    };
    let module_info = root.join("Contents").join("moduleinfo.json");
    let Ok(text) = fs::read_to_string(&module_info) else {
        return metadata;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return metadata;
    };
    // VST3 module info uses capitalised keys; accept any casing so a future
    // schema tweak does not break the wrapper.
    let field = |key: &str| -> Option<String> {
        value.as_object()?.iter().find_map(|(name, entry)| {
            name.eq_ignore_ascii_case(key)
                .then(|| entry.as_str().map(str::to_owned))
                .flatten()
        })
    };
    if let Some(name) = field("name") {
        metadata.name = name;
    }
    if let Some(vendor) = field("vendor") {
        metadata.vendor = vendor;
    }
    if let Some(version) = field("version") {
        metadata.version = version;
    }
    if let Some(classes) = value
        .get("Classes")
        .or_else(|| value.get("classes"))
        .and_then(|classes| classes.as_array())
    {
        for class in classes {
            if let Some(name) = class.get("Name").and_then(|name| name.as_str()) {
                metadata.classes.push(name.to_owned());
            }
            if let Some(category) = class.get("Category").and_then(|value| value.as_str()) {
                if !metadata.categories.iter().any(|item| item == category) {
                    metadata.categories.push(category.to_owned());
                }
            }
        }
    }
    metadata
}

/// Sample/stereo parameters handed to a hosted external plug-in.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostParameters {
    /// Dry/wet balance in 0..=1.
    pub mix: f32,
    /// Gain applied before the plug-in.
    pub input_gain: f32,
    /// Gain applied after the plug-in.
    pub output_gain: f32,
}

/// Boundary for external plug-in processing.
///
/// The shipped build has no third-party hosting backend, so the only provided
/// implementation is [`BypassHost`], which passes audio through unchanged while
/// keeping the slot's mix and gain automatable. A real VST3/CLAP backend
/// implements this trait and is selected when the slot carries a plug-in path.
pub trait PluginHost: Send {
    /// Processes one stereo frame in place.
    fn process(&mut self, frame: [f32; 2], parameters: HostParameters) -> [f32; 2];
}

/// Pass-through host used when no hosting backend is available.
#[derive(Clone, Copy, Debug, Default)]
pub struct BypassHost;

impl PluginHost for BypassHost {
    fn process(&mut self, frame: [f32; 2], parameters: HostParameters) -> [f32; 2] {
        let dry = [
            frame[0] * parameters.input_gain,
            frame[1] * parameters.input_gain,
        ];
        let mixed = [
            frame[0] + (dry[0] - frame[0]) * parameters.mix,
            frame[1] + (dry[1] - frame[1]) * parameters.mix,
        ];
        [
            mixed[0] * parameters.output_gain,
            mixed[1] * parameters.output_gain,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_for_display() {
        assert_eq!(clean_name("Surge_XT-x64"), "Surge XT");
        assert_eq!(clean_name("Vital x64"), "Vital");
        assert_eq!(clean_name("Simple"), "Simple");
    }

    #[test]
    fn bypass_host_keeps_signal_inside_the_requested_mix() {
        let mut host = BypassHost;
        let parameters = HostParameters {
            mix: 1.0,
            input_gain: 1.0,
            output_gain: 1.0,
        };
        assert_eq!(host.process([0.5, -0.5], parameters), [0.5, -0.5]);
        let muted = HostParameters {
            mix: 0.0,
            input_gain: 1.0,
            output_gain: 1.0,
        };
        assert_eq!(host.process([0.5, -0.5], muted), [0.5, -0.5]);
    }

    #[test]
    fn extensions_drive_format_detection() {
        assert!(PluginFormat::Vst2.extensions().contains(&"dll"));
        assert!(PluginFormat::Clap.extensions().contains(&"clap"));
        assert_eq!(PluginFormat::Vst3.label(), "VST3");
    }

    #[test]
    fn scanning_reports_bundles_with_their_format() {
        let root = std::env::temp_dir().join("reafl-plugin-scan-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Sub.vst3")).unwrap();
        fs::write(root.join("Test Synth.clap"), b"clap").unwrap();
        fs::write(root.join("Old Delay.dll"), b"vst").unwrap();
        fs::write(root.join("notes.txt"), b"ignored").unwrap();

        let plugins = scan_plugins(&[root.to_string_lossy().into_owned()], 16);

        assert!(
            plugins
                .iter()
                .any(|plugin| plugin.name == "Test Synth" && plugin.format == PluginFormat::Clap)
        );
        assert!(
            plugins
                .iter()
                .any(|plugin| plugin.name == "Sub" && plugin.format == PluginFormat::Vst3)
        );
        assert!(
            plugins
                .iter()
                .any(|plugin| plugin.name == "Old Delay" && plugin.format == PluginFormat::Vst2)
        );
        assert!(!plugins.iter().any(|plugin| plugin.name.contains("notes")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn vst3_module_info_is_parsed_from_the_bundle() {
        let root = std::env::temp_dir().join("reafl-vst3-metadata-test");
        let bundle = root.join("Demo.vst3");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(
            bundle.join("Contents").join("moduleinfo.json"),
            r#"{"Name":"Demo Synth","Vendor":"Example Audio","Version":"1.2.3",
                "Classes":[{"Name":"Demo Synth","Category":"Instrument|Synth"},
                           {"Name":"Demo FX","Category":"Fx"}]}"#,
        )
        .unwrap();

        let metadata = read_metadata(bundle.to_string_lossy().as_ref());

        assert_eq!(metadata.name, "Demo Synth");
        assert_eq!(metadata.vendor, "Example Audio");
        assert_eq!(metadata.version, "1.2.3");
        assert_eq!(metadata.classes, vec!["Demo Synth", "Demo FX"]);
        assert_eq!(metadata.categories, vec!["Instrument|Synth", "Fx"]);
        assert!(metadata.is_informative());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn metadata_falls_back_to_the_file_name() {
        let metadata = read_metadata("C:/nowhere/Some_Delay-x64.clap");
        assert_eq!(metadata.name, "Some Delay");
        assert!(!metadata.is_informative());
    }

    #[test]
    fn scanning_missing_directories_is_not_an_error() {
        let plugins = scan_plugins(&["Z:/definitely/not/here".to_owned()], 8);
        assert!(plugins.len() <= 8);
    }
}
