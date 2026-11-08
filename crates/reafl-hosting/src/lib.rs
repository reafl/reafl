//! VST3 and CLAP plug-in hosting.
//!
//! The two formats are driven by separate, well-tested crates (`vst3-host` and
//! `clack-host`); this crate wraps them behind one small API so the DAW never
//! depends on either format directly.
//!
//! A load returns two halves because the formats disagree on threading:
//!
//! * [`HostedUi`] stays on the UI thread and owns everything main-thread-only —
//!   the editor, the parameter list and the metadata shown in the wrapper.
//! * [`HostedAudio`] is `Send` and owns the audio side, so the real-time engine
//!   can move it onto the audio thread and render blocks there.
//!
//! VST3 shares one `Plugin` behind a mutex because the crate's own helpers do
//! the same; CLAP hands out a `Send` processing handle while the instance stays
//! on the UI thread.
//!
//! Native editor embedding is implemented for Windows only: [`editor::EditorWindow`]
//! creates a child `HWND` there and returns `None` everywhere else, so the
//! wrapper reports "could not create a native window" instead of embedding. The
//! VST3 handle wrapper still knows the macOS and X11 constructors, because the
//! VST3 platform type differs per OS even where no window is created yet.

pub mod clap;
pub mod editor;
pub mod vst3;

/// Result type used by the hosting layer.
pub type HostResult<T> = Result<T, HostError>;

#[derive(Debug, Clone)]
pub enum HostError {
    Vst3(String),
    Clap(String),
    /// The bundle has no editor, or it refused to attach one.
    NoEditor(String),
}

impl core::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Vst3(message) => write!(formatter, "VST3: {message}"),
            Self::Clap(message) => write!(formatter, "CLAP: {message}"),
            Self::NoEditor(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for HostError {}

/// Native parent window for an embedded editor (an `HWND` on Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParentWindow(pub isize);

/// Description shown in the wrapper header before the editor is attached.
#[derive(Clone, Debug, Default)]
pub struct BundleInfo {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub supports_editor: bool,
    pub audio_outputs: usize,
    pub accepts_midi: bool,
}

/// One automatable plug-in parameter, normalised to 0..=1.
#[derive(Clone, Debug)]
pub struct ParameterInfo {
    pub id: u32,
    pub name: String,
    pub value: f32,
    pub default: f32,
}

/// Loads a bundle and splits it into its UI and audio halves.
///
/// `sample_rate` and `block` are needed up front because CLAP can only be
/// activated while the non-`Send` instance is still on the caller's thread.
pub fn load(
    path: &str,
    instrument: bool,
    sample_rate: f64,
    block: usize,
) -> HostResult<(HostedUi, HostedAudio)> {
    if path.to_ascii_lowercase().ends_with(".clap") {
        let (ui, audio) = clap::load(path, instrument, sample_rate, block)?;
        Ok((HostedUi::Clap(ui), HostedAudio::Clap(audio)))
    } else {
        let (ui, audio) = vst3::load(path, instrument)?;
        Ok((HostedUi::Vst3(ui), HostedAudio::Vst3(audio)))
    }
}

/// UI-thread half of a hosted plug-in.
pub enum HostedUi {
    Vst3(vst3::Vst3Ui),
    Clap(clap::ClapUi),
}

/// Audio-thread half of a hosted plug-in. Safe to move across threads.
///
/// CLAP's variant is much larger than VST3's, so the enum is boxed at the call
/// site that moves it across threads.
#[allow(clippy::large_enum_variant)]
pub enum HostedAudio {
    Vst3(vst3::Vst3Audio),
    Clap(clap::ClapAudio),
}

impl HostedUi {
    pub fn info(&self) -> &BundleInfo {
        match self {
            Self::Vst3(ui) => ui.info(),
            Self::Clap(ui) => ui.info(),
        }
    }

    pub fn parameters(&self) -> Vec<ParameterInfo> {
        match self {
            Self::Vst3(ui) => ui.parameters(),
            Self::Clap(ui) => ui.parameters(),
        }
    }

    /// Writes a normalized value straight through the instance (the UI path,
    /// used for knob drags). Audio-rate automation uses
    /// [`HostedAudio::set_parameter`] instead so it lands inside a block.
    pub fn set_parameter(&mut self, id: u32, value: f32) {
        match self {
            Self::Vst3(ui) => ui.set_parameter(id, value),
            Self::Clap(ui) => ui.set_parameter(id, value),
        }
    }

    /// Opaque plug-in state, for saving it in the project.
    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Vst3(ui) => ui.save_state(),
            Self::Clap(ui) => ui.save_state(),
        }
    }

    /// Restores a previously saved state blob.
    pub fn load_state(&mut self, data: &[u8]) -> bool {
        match self {
            Self::Vst3(ui) => ui.load_state(data),
            Self::Clap(ui) => ui.load_state(data),
        }
    }

    /// Writes a `.vstpreset`/raw state file, when the format supports it.
    pub fn save_preset(&mut self, path: &str) -> HostResult<()> {
        match self {
            Self::Vst3(ui) => ui.save_preset(path),
            Self::Clap(ui) => ui.save_preset(path),
        }
    }

    /// Last parameter the plug-in's own editor changed, with its value.
    pub fn take_last_tweaked(&mut self) -> Option<(u32, f32)> {
        match self {
            Self::Vst3(ui) => ui.take_last_tweaked(),
            Self::Clap(ui) => ui.take_last_tweaked(),
        }
    }

    /// Resizes the attached editor; `None` when it is closed or refuses.
    pub fn resize_editor(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        match self {
            Self::Vst3(ui) => ui.resize_editor(width, height),
            Self::Clap(ui) => ui.resize_editor(width, height),
        }
    }

    /// Editor size in physical pixels, when the format can report it before the
    /// editor exists.
    pub fn editor_size(&mut self) -> Option<(u32, u32)> {
        match self {
            Self::Vst3(ui) => ui.editor_size(),
            Self::Clap(ui) => ui.editor_size(),
        }
    }

    /// Attaches the editor and reports its real size (plug-ins may ignore the
    /// requested one).
    pub fn open_editor(
        &mut self,
        parent: ParentWindow,
        width: u32,
        height: u32,
    ) -> HostResult<(u32, u32)> {
        match self {
            Self::Vst3(ui) => ui.open_editor(parent, width, height),
            Self::Clap(ui) => ui.open_editor(parent, width, height),
        }
    }

    pub fn close_editor(&mut self) {
        match self {
            Self::Vst3(ui) => ui.close_editor(),
            Self::Clap(ui) => ui.close_editor(),
        }
    }
}

impl HostedAudio {
    /// Prepares the instance for `block`-frame processing at `sample_rate`.
    pub fn activate(&mut self, sample_rate: f64, block: usize) {
        match self {
            Self::Vst3(audio) => audio.activate(sample_rate, block),
            Self::Clap(audio) => audio.activate(sample_rate, block),
        }
    }

    pub fn set_parameter(&mut self, id: u32, value: f32) {
        match self {
            Self::Vst3(audio) => audio.set_parameter(id, value),
            Self::Clap(audio) => audio.set_parameter(id, value),
        }
    }

    pub fn send_note(&mut self, note: u8, velocity: f32, on: bool) {
        match self {
            Self::Vst3(audio) => audio.send_note(note, velocity, on),
            Self::Clap(audio) => audio.send_note(note, velocity, on),
        }
    }

    /// Renders exactly one block in place; `left` and `right` must be the block
    /// length given to [`Self::activate`].
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self {
            Self::Vst3(audio) => audio.process(left, right),
            Self::Clap(audio) => audio.process(left, right),
        }
    }

    /// Clears voices and processing state (transport stop).
    pub fn reset(&mut self) {
        match self {
            Self::Vst3(audio) => audio.reset(),
            Self::Clap(audio) => audio.reset(),
        }
    }
}
