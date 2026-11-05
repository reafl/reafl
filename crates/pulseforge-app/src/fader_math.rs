//! The fader's dB scale and its silence snap.


/// Converts a linear gain to decibels, returning [`FADER_MIN_DB`] for silence.
///
/// # Parameters
///
/// - `gain`: linear gain, where `1.0` is unity.
///
/// # Returns
///
/// The level in decibels, clamped at the bottom of the fader's travel.
pub(crate) fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.00001 {
        FADER_MIN_DB
    } else {
        20.0 * gain.log10()
    }
}

/// Lowest level a mixer fader reaches before it snaps to silence, in dB.
pub(crate) const FADER_MIN_DB: f32 = -100.0;

/// Fader travel position for a linear gain.
///
/// # Parameters
///
/// - `gain`: linear gain.
///
/// # Returns
///
/// `0.0` at or below silence and `1.0` at unity gain.
pub(crate) fn fader_position(gain: f32) -> f32 {
    if gain <= 0.0 {
        0.0
    } else {
        (gain_to_db(gain) / FADER_MIN_DB).clamp(0.0, 1.0)
    }
}

/// Linear gain for a fader position. The bottom of the travel is exactly zero,
/// which is the "snap to 0" step at the end of the dB scale.
///
/// # Parameters
///
/// - `position`: fader travel, `0.0` to `1.0`.
///
/// # Returns
///
/// The linear gain the handle position stands for.
pub(crate) fn fader_gain(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    if position <= 0.0 {
        0.0
    } else {
        10f32.powf(position * FADER_MIN_DB / 20.0)
    }
}

/// Fader read-out: finite dB values, and the infinity sign for silence.
///
/// # Parameters
///
/// - `gain`: linear gain.
///
/// # Returns
///
/// Text for the strip label, for example `"-6.0 dB"` or `"-∞ dB"`.
pub(crate) fn fader_label(gain: f32) -> String {
    if gain <= 0.0 {
        "-\u{221E} dB".to_owned()
    } else {
        format!("{:.1} dB", gain_to_db(gain))
    }
}
