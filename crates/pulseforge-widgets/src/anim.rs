//! Small UI transition helpers.
//!
//! egui redraws on demand, so animation is explicit: every animated value eases
//! toward a target each frame and requests another frame until it arrives. The
//! helpers here keep that logic in one place so the widgets stay readable and a
//! single setting can disable every transition.

/// An eased scalar that chases a target value.
#[derive(Clone, Copy, Debug)]
pub struct Animated {
    current: f32,
    target: f32,
    /// Higher is snappier. 14 is a quick UI response at 60 Hz.
    stiffness: f32,
}

impl Animated {
/// Creates an eased value that starts settled at `value`.
///
/// # Parameters
///
/// - `value`: the starting value.
///
/// # Returns
///
/// An animation already at rest.
    pub fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            stiffness: 14.0,
        }
    }

/// Sets the target the value eases toward.
///
/// # Parameters
///
/// - `target`: the value to reach.
    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Advances toward the target. `dt` is the frame time in seconds.
    pub fn step(&mut self, dt: f32, enabled: bool, speed: f32) -> f32 {
        if !enabled {
            self.current = self.target;
            return self.current;
        }
        let dt = dt.clamp(0.0, 0.1);
        let response = 1.0 - (-self.stiffness * speed.max(0.05) * dt).exp();
        self.current += (self.target - self.current) * response;
        if (self.target - self.current).abs() < 0.0005 {
            self.current = self.target;
        }
        self.current
    }

/// The value as it stands this frame.
///
/// # Returns
///
/// The current, possibly still moving, value.
    pub fn value(&self) -> f32 {
        self.current
    }

    /// True while the value has not reached its target, so the caller knows to
    /// request another repaint.
    pub fn is_animating(&self) -> bool {
        (self.target - self.current).abs() > 0.0005
    }
}

impl Default for Animated {
    fn default() -> Self {
        Self::new(0.0)
    }
}

/// Eases a value between 0 and 1 with a smoothstep curve.
pub fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_value_reaches_its_target() {
        let mut value = Animated::new(0.0);
        value.set(1.0);
        for _ in 0..600 {
            value.step(1.0 / 60.0, true, 1.0);
        }
        assert!((value.value() - 1.0).abs() < 0.001);
        assert!(!value.is_animating());
    }

    #[test]
    fn disabled_animation_snaps() {
        let mut value = Animated::new(0.0);
        value.set(1.0);
        assert!((value.step(1.0 / 60.0, false, 1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn smoothstep_is_clamped_and_symmetric() {
        assert!((smoothstep(-1.0) - 0.0).abs() < f32::EPSILON);
        assert!((smoothstep(2.0) - 1.0).abs() < f32::EPSILON);
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-6);
    }
}
