param([string]$Root = "crates/pulseforge-core/src")

$source = Join-Path $Root 'audio.rs'
$target = Join-Path $Root 'audio'
if (-not (Test-Path $target)) { New-Item -ItemType Directory -Force $target | Out-Null }

$lines = Get-Content -LiteralPath $source
$head = $lines[0..27]          # module docs and the shared imports
$model = $lines[28..473]       # data types, ending before AudioEngine
$engine = $lines[474..2596]    # AudioEngine and its methods
$dsp = $lines[2597..2887]      # free DSP helpers and WAV I/O
$tests = $lines[2888..($lines.Count - 1)]

function Widen([string[]]$Content) {
    $out = @()
    foreach ($line in $Content) {
        if ($line -match '^(struct|enum|const|fn) ') {
            $out += ($line -replace '^(struct|enum|const|fn) ', 'pub(crate) $1 ')
        }
        else { $out += $line }
    }
    return $out
}

$imports = $head | Where-Object { $_ -match '^use |^    |^\};|^$' -and $_ -notmatch '^//!' }

Set-Content -LiteralPath (Join-Path $target 'model.rs') -Value (@(
    '//! Audio-side data: samples, commands, voices and the engine state.',
    ''
) + $head + @('') + (Widen $model))

Set-Content -LiteralPath (Join-Path $target 'engine.rs') -Value (@(
    '//! The render engine: mixing, automation application and effect chains.',
    '',
    'use super::model::*;',
    'use super::dsp::*;',
    ''
) + (Widen $engine))

Set-Content -LiteralPath (Join-Path $target 'dsp.rs') -Value (@(
    '//! Free DSP helpers: oscillators, EQ, sample interpolation and WAV I/O.',
    '',
    'use super::model::*;',
    'use super::engine::*;',
    ''
) + (Widen $dsp))

Set-Content -LiteralPath (Join-Path $target 'tests.rs') -Value (@(
    '//! Tests for the engine and its DSP helpers.',
    '',
    'use super::model::*;',
    'use super::engine::*;',
    'use super::dsp::*;',
    ''
) + $tests)

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
Write-Host "audio split: model $($model.Count), engine $($engine.Count), dsp $($dsp.Count), tests $($tests.Count)"
