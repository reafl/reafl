//! Parameter smoothing.
//!
//! Mixer and plug-in values are usually set by discontinuous events: a knob
//! drag jumps in steps, automation lands on a new value each control block, and
//! a preset recall can move every parameter at once. Applying those jumps
//! directly produces zipper noise, so the audio path interpolates linearly
//! toward the target value instead. The behaviour is intentionally simple and
//! deterministic so it can be disabled from the settings window.

/// One-pole-free linear ramp toward a target value.
///
/// The ramp length is expressed in seconds and converted to samples once, so
/// per-sample work is a single multiply-add.
#[derive(Clone, Copy, Debug)]
pub struct SmoothedValue {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    /// Ramp length in samples, applied whenever a new target is set.
    ramp_samples: u32,
}

impl SmoothedValue {
    /// Creates a smoother that starts at `value` and ramps over `samples`.
    pub fn new(value: f32, ramp_samples: u32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            ramp_samples: ramp_samples.max(1),
        }
    }

    /// Replaces the configured ramp length, keeping the current value.
    pub fn with_ramp(mut self, ramp_samples: u32) -> Self {
        self.ramp_samples = ramp_samples.max(1);
        self.remaining = 0;
        self
    }

    /// Sets a new target. Re-targeting restarts the ramp from the current value
    /// so a moving knob never jumps backwards.
    pub fn set_target(&mut self, target: f32) {
        if (target - self.target).abs() <= f32::EPSILON {
            return;
        }
        self.target = target;
        self.remaining = self.ramp_samples;
        self.step = (self.target - self.current) / self.remaining as f32;
    }

    /// Snaps to the target immediately, used when smoothing is disabled.
    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
        self.step = 0.0;
    }

    /// Advances one sample and returns the value to use.
    pub fn advance(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            } else {
                self.current += self.step;
            }
        }
        self.current
    }

    /// Current value without advancing.
    pub fn value(&self) -> f32 {
        self.current
    }

    /// Target value the ramp is heading toward.
    pub fn target(&self) -> f32 {
        self.target
    }
}

impl Default for SmoothedValue {
    fn default() -> Self {
        Self::new(0.0, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_reaches_the_target_without_overshoot() {
        let mut value = SmoothedValue::new(0.0, 4);
        value.set_target(1.0);
        let samples: Vec<f32> = (0..5).map(|_| value.advance()).collect();
        assert!((samples[0] - 0.25).abs() < 1e-6);
        assert!((samples[3] - 1.0).abs() < 1e-6);
        assert!((value.value() - 1.0).abs() < 1e-6);
        assert!(samples.iter().all(|sample| (0.0..=1.0).contains(sample)));
    }

    #[test]
    fn retargeting_restarts_from_the_current_value() {
        let mut value = SmoothedValue::new(0.0, 4);
        value.set_target(1.0);
        let _ = value.advance();
        let midway = value.value();
        value.set_target(0.0);
        assert!((value.value() - midway).abs() < 1e-6);
        let next = value.advance();
        assert!(next < midway, "ramp should move toward the new target");
        assert!((next - midway).abs() < 0.5);
    }

    #[test]
    fn snap_skips_the_ramp() {
        let mut value = SmoothedValue::new(0.0, 64);
        value.set_target(0.8);
        value.snap(0.8);
        assert!((value.advance() - 0.8).abs() < 1e-6);
    }
}
