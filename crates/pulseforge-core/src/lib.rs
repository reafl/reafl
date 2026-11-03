pub mod audio;
pub mod hosted;
/// Re-export of the plug-in layer so existing call sites keep one import path.
pub mod plugins {
    pub use pulseforge_plugins::{
        BUILTIN_EFFECT_NAMES, BuiltinEffect, BypassHost, EFFECTS, EffectParameters, ExternalPlugin,
        GENERATORS, HostParameters, PluginDescriptor, PluginFormat, PluginHost, effect_by_name,
    };
}
pub mod project;
pub mod smooth;
