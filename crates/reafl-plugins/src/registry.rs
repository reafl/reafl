//! Built-in plug-in descriptors.
//!
//! The registry is the single place that knows which built-in processors exist.
//! The UI lists them, the audio engine instantiates them by name, and the
//! settings window can show them for diagnostics, so adding a processor means
//! touching one module instead of every call site.

/// A generator (instrument) or effect descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginDescriptor {
    /// Stable identifier used in project files and automation targets.
    pub id: &'static str,
    /// Text shown in the plug-in picker and window titles.
    pub name: &'static str,
    /// Whether the plug-in generates audio or processes it.
    pub kind: PluginKind,
    /// One-line description for the picker tooltip.
    pub summary: &'static str,
}

/// What a built-in plug-in does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginKind {
    /// Instrument: turns notes into audio.
    Generator,
    /// Effect: processes the channel's audio.
    Effect,
}

/// Built-in generators, in picker order.
pub const GENERATORS: &[PluginDescriptor] = &[
    PluginDescriptor {
        id: "sample_player",
        name: "Sample Player",
        kind: PluginKind::Generator,
        summary: "Plays one-shot samples with gain and pan automation.",
    },
    PluginDescriptor {
        id: "sine",
        name: "Sine Synth",
        kind: PluginKind::Generator,
        summary: "Single-oscillator sine voice.",
    },
    PluginDescriptor {
        id: "signal_generator",
        name: "Signal Generator",
        kind: PluginKind::Generator,
        summary: "Sine, triangle, saw and square test oscillator.",
    },
    PluginDescriptor {
        id: "ms20",
        name: "MS-20 Mono",
        kind: PluginKind::Generator,
        summary: "Monophonic dual-saw voice with a resonant low-pass.",
    },
    PluginDescriptor {
        id: "juno",
        name: "Juno Poly",
        kind: PluginKind::Generator,
        summary: "Polyphonic saw/pulse/sub chorus voice.",
    },
    PluginDescriptor {
        id: "m1",
        name: "M1 Workstation",
        kind: PluginKind::Generator,
        summary: "Additive workstation voice with a decay envelope.",
    },
    PluginDescriptor {
        id: "fm",
        name: "DX7 FM",
        kind: PluginKind::Generator,
        summary: "Two-operator FM voice with ratio and index control.",
    },
];

/// Built-in effects, in picker order.
pub const EFFECTS: &[PluginDescriptor] = &[
    PluginDescriptor {
        id: "3_band_eq",
        name: "3-Band EQ",
        kind: PluginKind::Effect,
        summary: "Low, mid and high shelving EQ.",
    },
    PluginDescriptor {
        id: "reverb",
        name: "Reverb",
        kind: PluginKind::Effect,
        summary: "Algorithmic room reverb.",
    },
    PluginDescriptor {
        id: "plate_reverb",
        name: "Plate Reverb",
        kind: PluginKind::Effect,
        summary: "Bright plate-style reverb.",
    },
    PluginDescriptor {
        id: "delay",
        name: "Delay",
        kind: PluginKind::Effect,
        summary: "Tempo-aware stereo delay.",
    },
    PluginDescriptor {
        id: "tape_echo",
        name: "Tape Echo",
        kind: PluginKind::Effect,
        summary: "Delay with a low-pass feedback path.",
    },
    PluginDescriptor {
        id: "chorus",
        name: "Juno Chorus",
        kind: PluginKind::Effect,
        summary: "Slow modulated stereo chorus.",
    },
    PluginDescriptor {
        id: "phaser",
        name: "Phaser",
        kind: PluginKind::Effect,
        summary: "Four-stage modulated all-pass phaser.",
    },
    PluginDescriptor {
        id: "waveshaper",
        name: "Waveshaper",
        kind: PluginKind::Effect,
        summary: "Soft-clipping drive stage.",
    },
];

/// Finds a descriptor by its display name.
pub fn effect_by_name(name: &str) -> Option<&'static PluginDescriptor> {
    EFFECTS.iter().find(|effect| effect.name == name)
}

/// Finds a descriptor by its stable identifier.
pub fn effect_by_id(id: &str) -> Option<&'static PluginDescriptor> {
    EFFECTS.iter().find(|effect| effect.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_effect_name_has_a_descriptor() {
        for name in crate::builtin::BUILTIN_EFFECT_NAMES {
            assert!(
                effect_by_name(name).is_some(),
                "missing descriptor for {name}"
            );
        }
    }

    #[test]
    fn descriptor_ids_are_unique_per_kind() {
        let mut ids: Vec<&str> = EFFECTS.iter().map(|effect| effect.id).collect();
        ids.extend(GENERATORS.iter().map(|generator| generator.id));
        let unique = ids.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), ids.len());
    }
}
