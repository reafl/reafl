//! PulseForge plug-in layer.
//!
//! * [`builtin`] holds the DSP for the included processors.
//! * [`registry`] lists what exists so the UI and engine share one source of
//!   truth.
//! * [`external`] discovers VST/VST3/CLAP bundles and defines the hosting
//!   boundary for them.
//!
//! The crate deliberately has no dependency on the project model: projects
//! reference plug-ins by name or path, and conversion lives in the audio crate.
//! That keeps the DSP reusable and avoids a dependency cycle.

#![warn(missing_docs)]

pub mod builtin;
pub mod external;
pub mod registry;

pub use builtin::{BUILTIN_EFFECT_NAMES, BuiltinEffect, EffectParameters};
pub use external::{
    BypassHost, ExternalPlugin, HostParameters, PluginFormat, PluginHost, PluginMetadata,
    default_plugin_directories, read_metadata, scan_plugins,
};
pub use registry::{
    EFFECTS, GENERATORS, PluginDescriptor, PluginKind, effect_by_id, effect_by_name,
};
