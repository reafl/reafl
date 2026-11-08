param([string]$Root = "crates/reafl-core/src")

$source = Join-Path $Root 'audio.rs'
$target = Join-Path $Root 'audio'
if (-not (Test-Path $target)) { New-Item -ItemType Directory -Force $target | Out-Null }

$lines = Get-Content -LiteralPath $source

# The module already separates into device/voice types, the engine itself, the
# free DSP helpers and the tests.
$valuesEnd = 474
$engineEnd = 2597
$helpersEnd = 2888

Set-Content -LiteralPath (Join-Path $target 'model.rs') -Value (@(
    '//! The audio-side data: samples, commands, voices and the engine state.'
) + $lines[0..($valuesEnd - 1)])

Set-Content -LiteralPath (Join-Path $target 'engine.rs') -Value (@(
    '//! The render engine: mixing, automation application and effect chains.',
    '',
    'use super::*;',
    ''
) + $lines[$valuesEnd..($engineEnd - 1)])

Set-Content -LiteralPath (Join-Path $target 'dsp.rs') -Value (@(
    '//! Free DSP helpers: oscillators, EQ, sample interpolation and WAV I/O.',
    '',
    'use super::*;',
    ''
) + $lines[$engineEnd..($helpersEnd - 1)])

Set-Content -LiteralPath (Join-Path $target 'tests.rs') -Value (@(
    '//! Tests for the engine and its DSP helpers.',
    '',
    'use super::*;',
    ''
) + $lines[$helpersEnd..($lines.Count - 1)])

Set-Content -LiteralPath (Join-Path $target 'mod.rs') -Value @(
    '//! The audio engine: device handling, rendering and file I/O.',
    '//!',
    '//! [`AudioOutput`] owns the cpal stream and the command queue the UI talks to;',
    '//! [`AudioEngine`] is the renderer that runs on the audio thread. The data they',
    '//! share lives in [`model`], the renderer in [`engine`], and the free DSP',
    '//! helpers in [`dsp`].',
    '',
    'mod dsp;',
    'mod engine;',
    'mod model;',
    '',
    '#[cfg(test)]',
    'mod tests;',
    '',
    'pub use dsp::*;',
    'pub use engine::*;',
    'pub use model::*;'
)

Remove-Item -LiteralPath $source
Write-Host 'audio.rs split into audio/{mod,model,engine,dsp,tests}.rs'
