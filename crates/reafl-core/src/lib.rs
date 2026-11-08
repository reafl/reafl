//! Core data model and audio engine of reafl.
//!
//! The crate is deliberately UI-free: [`project`] holds the document model and
//! its behaviour, [`audio`] renders it, [`expr`] evaluates the automation
//! formula language and [`hosted`] bridges the plug-in layer.

#![warn(missing_docs)]

pub mod audio;
pub mod expr;
pub mod hosted;
/// Re-export of the plug-in layer so existing call sites keep one import path.
pub mod plugins {
    pub use reafl_plugins::{
        BUILTIN_EFFECT_NAMES, BuiltinEffect, BypassHost, EFFECTS, EffectParameters, ExternalPlugin,
        GENERATORS, HostParameters, PluginDescriptor, PluginFormat, PluginHost, effect_by_name,
    };
}
pub mod project;
pub mod smooth;
