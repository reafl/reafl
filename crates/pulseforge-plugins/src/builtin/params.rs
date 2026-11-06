//! The parameter block shared by every built-in effect, and its clamps.

/// The parameters an insert slot passes to its processor.
#[derive(Clone, Copy, Debug, Default)]
pub struct EffectParameters {
    /// Dry/wet balance, `0` dry to `1` fully wet.
    pub mix: f32,
    /// Effect strength; meaning depends on the processor.
    pub amount: f32,
    /// Low shelf gain in dB.
    pub low_db: f32,
    /// Mid band gain in dB.
    pub mid_db: f32,
    /// High shelf gain in dB.
    pub high_db: f32,
    /// Delay time in seconds.
    pub time_seconds: f32,
    /// Delay feedback, `0` to `1`.
    pub feedback: f32,
    /// Modulation rate in Hz.
    pub rate_hz: f32,
    /// Modulation depth, `0` to `1`.
    pub depth: f32,
    /// Reverb room size, `0` to `1`.
    pub room_size: f32,
    /// Waveshaper drive, `0` to `1`.
    pub drive: f32,
}

/// Clamps a parameter block into the ranges the processors assume.
///
/// # Parameters
///
/// - `parameters`: the values coming from the project or from automation.
///
/// # Returns
///
/// The same block with every field clamped and non-finite values replaced.
pub fn sanitize(mut parameters: EffectParameters) -> EffectParameters {
    parameters.mix = finite_clamp(parameters.mix, 0.0, 1.0);
    parameters.amount = finite_clamp(parameters.amount, 0.0, 1.0);
    parameters.low_db = finite_clamp(parameters.low_db, -12.0, 12.0);
    parameters.mid_db = finite_clamp(parameters.mid_db, -12.0, 12.0);
    parameters.high_db = finite_clamp(parameters.high_db, -12.0, 12.0);
    parameters.time_seconds = finite_clamp(parameters.time_seconds, 0.01, 1.0);
    parameters.feedback = finite_clamp(parameters.feedback, 0.0, 0.95);
    parameters.rate_hz = finite_clamp(parameters.rate_hz, 0.05, 8.0);
    parameters.depth = finite_clamp(parameters.depth, 0.0, 1.0);
    parameters.room_size = finite_clamp(parameters.room_size, 0.0, 1.0);
    parameters.drive = finite_clamp(parameters.drive, 0.0, 1.0);
    parameters
}

/// Clamps `value` into `min..=max`, replacing non-finite values with `min`.
///
/// # Parameters
///
/// - `value`: the value to clamp.
/// - `min`: lower bound.
/// - `max`: upper bound.
///
/// # Returns
///
/// A finite value inside the range.
pub fn finite_clamp(value: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    }
}

/// Converts a gain in decibels to a linear multiplier.
///
/// # Parameters
///
/// - `db`: the gain in decibels.
///
/// # Returns
///
/// The linear gain that dB value stands for.
pub fn db_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}
