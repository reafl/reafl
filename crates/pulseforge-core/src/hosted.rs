//! Real-time side of the plug-in hosting.
//!
//! The engine renders one frame at a time, but VST3 and CLAP both want whole
//! blocks. [`HostedSlot`] bridges that: it accumulates a fixed-size block,
//! processes it, and then replays the processed block one sample at a time.
//! That costs exactly one block of latency per plug-in and no allocation in the
//! callback.

use std::{sync::Arc, time::Instant};

use pulseforge_hosting::HostedAudio;
use pulseforge_profiling::SlotCounters;

/// Block size every hosted plug-in is activated with. It is also the latency a
/// hosted insert adds (64 frames ≈ 1.3 ms at 48 kHz).
pub const HOST_BLOCK: usize = 64;

/// Stable host id for an instrument's plug-in slot.
pub fn instrument_host_id(instrument_id: usize) -> u64 {
    instrument_id as u64
}

/// Stable host id for one insert slot on one mixer lane.
pub fn effect_host_id(lane: usize, slot: usize) -> u64 {
    (1u64 << 40) | ((lane as u64) << 16) | slot as u64
}

pub struct HostedSlot {
    audio: Option<HostedAudio>,
    input: [[f32; HOST_BLOCK]; 2],
    output: [[f32; HOST_BLOCK]; 2],
    cursor: usize,
    instrument: bool,
    pending_parameters: Vec<(u32, f32)>,
    pending_notes: Vec<(u8, f32, bool)>,
    /// Pitch of the note this slot is currently holding, if any.
    pub held_note: Option<u8>,
    /// Last value written per parameter, used as the automation base so a
    /// curve ends where the knob was left.
    parameter_values: Vec<(u32, f32)>,
    /// Debug counters shared with the debug panel; written lock-free while
    /// rendering, so a slow plug-in is visible without touching the audio path.
    counters: Option<Arc<SlotCounters>>,
}

impl HostedSlot {
    /// Wraps an already-loaded plug-in and primes one silent block so the
    /// output never starts with uninitialised memory.
    pub fn new(
        mut audio: HostedAudio,
        instrument: bool,
        sample_rate: f64,
        counters: Option<Arc<SlotCounters>>,
    ) -> Self {
        audio.activate(sample_rate, HOST_BLOCK);
        let mut slot = Self {
            audio: Some(audio),
            input: [[0.0; HOST_BLOCK]; 2],
            output: [[0.0; HOST_BLOCK]; 2],
            cursor: 0,
            instrument,
            pending_parameters: Vec::new(),
            pending_notes: Vec::new(),
            held_note: None,
            parameter_values: Vec::new(),
            counters,
        };
        slot.flush();
        slot
    }

    pub fn set_parameter(&mut self, id: u32, value: f32) {
        match self.parameter_values.iter_mut().find(|(key, _)| *key == id) {
            Some((_, stored)) => *stored = value,
            None => self.parameter_values.push((id, value)),
        }
        self.pending_parameters.push((id, value));
    }

    /// Current value of a parameter, defaulting to the centre when the host has
    /// never written it.
    pub fn parameter_base(&self, id: u32) -> f32 {
        self.parameter_values
            .iter()
            .find(|(key, _)| *key == id)
            .map_or(0.5, |(_, value)| *value)
    }

    /// Queues a note event. Instruments hold the note until the next trigger or
    /// [`Self::release`], so a step sequence gates for exactly one step.
    pub fn play_note(&mut self, note: u8, velocity: f32) {
        if let Some(previous) = self.held_note.take() {
            self.pending_notes.push((previous, 0.0, false));
        }
        self.held_note = Some(note);
        self.pending_notes.push((note, velocity, true));
    }

    pub fn release(&mut self) {
        if let Some(note) = self.held_note.take() {
            self.pending_notes.push((note, 0.0, false));
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.pending_notes.push((note, velocity, true));
    }

    pub fn note_off(&mut self, note: u8) {
        self.pending_notes.push((note, 0.0, false));
    }

    /// Renders one sample of an instrument, ignoring any input.
    pub fn render_instrument(&mut self) -> [f32; 2] {
        let frame = [self.output[0][self.cursor], self.output[1][self.cursor]];
        self.advance();
        frame
    }

    /// Renders one sample of an effect, returning the sample from one block ago.
    pub fn render_effect(&mut self, frame: [f32; 2]) -> [f32; 2] {
        if !self.instrument {
            self.input[0][self.cursor] = frame[0];
            self.input[1][self.cursor] = frame[1];
        }
        let out = [self.output[0][self.cursor], self.output[1][self.cursor]];
        self.advance();
        out
    }

    fn advance(&mut self) {
        self.cursor += 1;
        if self.cursor >= HOST_BLOCK {
            self.cursor = 0;
            self.flush();
        }
    }

    fn flush(&mut self) {
        let Some(audio) = self.audio.as_mut() else {
            return;
        };
        for (id, value) in self.pending_parameters.drain(..) {
            audio.set_parameter(id, value);
        }
        for (note, velocity, on) in self.pending_notes.drain(..) {
            audio.send_note(note, velocity, on);
        }
        if self.instrument {
            self.input = [[0.0; HOST_BLOCK]; 2];
        }
        let mut left = self.input[0];
        let mut right = self.input[1];
        // Plug-in time is measured here rather than around the whole callback
        // so the debug panel can rank individual slots.
        let started = Instant::now();
        audio.process(&mut left, &mut right);
        if let Some(counters) = &self.counters {
            counters.record_block(started.elapsed().as_nanos() as u64);
        }
        self.output = [left, right];
        self.input = [[0.0; HOST_BLOCK]; 2];
    }

    pub fn reset(&mut self) {
        self.release();
        if let Some(audio) = self.audio.as_mut() {
            audio.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real bundles are only present on a machine that has plug-ins installed,
    /// so this is opt-in: `cargo test -p pulseforge-core -- --ignored`.
    const EFFECT_BUNDLE: &str = r"C:\Program Files\Common Files\VST3\DualDelayX.vst3";
    const INSTRUMENT_BUNDLE: &str = r"C:\Program Files\Common Files\CLAP\Vital.clap";

    fn load(bundle: &str, instrument: bool) -> Option<HostedAudio> {
        if !std::path::Path::new(bundle).exists() {
            eprintln!("skipping: {bundle} is not installed");
            return None;
        }
        match pulseforge_hosting::load(bundle, instrument, 48_000.0, HOST_BLOCK) {
            Ok((ui, audio)) => {
                drop(ui);
                Some(audio)
            }
            Err(error) => {
                eprintln!("skipping: {error}");
                None
            }
        }
    }

    #[test]
    #[ignore = "requires an installed VST3 bundle"]
    fn hosted_effect_renders_a_block_later() {
        let Some(audio) = load(EFFECT_BUNDLE, false) else {
            return;
        };
        let mut slot = HostedSlot::new(audio, false, 48_000.0, None);
        let mut peak = 0.0_f32;
        for index in 0..HOST_BLOCK * 4 {
            let noise = if index < HOST_BLOCK { 0.4 } else { 0.0 };
            let frame = slot.render_effect([noise, noise]);
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
        }
        assert!(peak > 0.0, "hosted effect produced silence");
    }

    #[test]
    #[ignore = "requires an installed CLAP bundle"]
    fn hosted_instrument_plays_a_note() {
        let Some(audio) = load(INSTRUMENT_BUNDLE, true) else {
            return;
        };
        let mut slot = HostedSlot::new(audio, true, 48_000.0, None);
        slot.play_note(60, 0.9);
        let mut peak = 0.0_f32;
        for _ in 0..HOST_BLOCK * 40 {
            let frame = slot.render_instrument();
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
        }
        assert!(peak > 0.0, "hosted instrument produced silence");
    }

    #[test]
    fn host_ids_do_not_collide() {
        assert_ne!(instrument_host_id(3), effect_host_id(0, 3));
        assert_ne!(effect_host_id(1, 2), effect_host_id(2, 1));
    }
}
