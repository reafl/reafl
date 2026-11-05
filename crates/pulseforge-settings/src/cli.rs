//! Command-line options.
//!
//! Every switch is optional: with no arguments PulseForge behaves exactly as
//! before and uses `settings.json` and `session.json` in the working directory.
//! `clap` handles parsing, `--help` and error messages.

use std::path::PathBuf;

use clap::Parser;

/// Startup configuration chosen on the command line.
#[derive(Debug, Clone, Default, Parser)]
#[command(
    name = "pulseforge",
    version,
    about = "PulseForge · a Rust-native pattern-based digital audio workstation",
    long_about = "PulseForge is a pattern-based DAW.\n\n\
                  Options let you pin the settings and project files, and open the \
                  in-editor profiler, which is useful while developing plug-ins."
)]
pub struct Cli {
    /// Settings file to read at startup and write when settings are saved.
    #[arg(short = 's', long, value_name = "FILE")]
    pub settings: Option<PathBuf>,
    /// Project file to load at startup; saves go back to this path.
    #[arg(short = 'p', long, value_name = "FILE")]
    pub project: Option<PathBuf>,
    /// Start with the debug/profiler panel open.
    #[arg(short = 'd', long)]
    pub debug: bool,
    /// Start without opening the audio device. Hosted plug-ins stay unloaded,
    /// which makes it safe to inspect a project that crashes a plug-in.
    #[arg(long)]
    pub no_audio: bool,
}

impl Cli {
    /// Path used for `Settings::load`/`save`.
    pub fn settings_path(&self) -> String {
        self.settings
            .as_ref()
            .map_or_else(|| crate::settings::SETTINGS_FILE.to_owned(), |path| {
                path.to_string_lossy().into_owned()
            })
    }

    /// Project file to load once the window exists, if one was given.
    pub fn project_path(&self) -> Option<String> {
        self.project
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults_keep_the_working_directory_files() {
        let cli = Cli::parse_from(["pulseforge"]);
        assert_eq!(cli.settings_path(), "settings.json");
        assert!(cli.project_path().is_none());
        assert!(!cli.debug);
        assert!(!cli.no_audio);
    }

    #[test]
    fn paths_and_flags_are_parsed() {
        let cli = Cli::parse_from([
            "pulseforge",
            "--settings",
            "cfg/my.json",
            "-p",
            "song.json",
            "--debug",
        ]);
        assert_eq!(cli.settings_path(), "cfg/my.json");
        assert_eq!(cli.project_path().as_deref(), Some("song.json"));
        assert!(cli.debug);
    }
}
