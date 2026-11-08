param([string]$Root = "crates/reafl-core/src")

# Rebuilds crates/reafl-core/src/audio.rs into audio/{mod,model,engine,dsp,tests}.rs.
# Run with -Restore to put audio.rs back first.

$source = Join-Path $Root 'audio.rs'
$target = Join-Path $Root 'audio'
if (-not (Test-Path $target)) { New-Item -ItemType Directory -Force $target | Out-Null }

$lines = Get-Content -LiteralPath $source
$head = $lines[0..27]
$model = $lines[28..473]
$engine = $lines[474..2596]
$dsp = $lines[2597..2887]
$tests = $lines[2888..($lines.Count - 1)]

# Prefixes crate-level items and, inside struct bodies, the fields themselves.
function Widen([string[]]$Content) {
    $out = @()
    $inStruct = $false
    foreach ($line in $Content) {
        if ($line -match '^(pub\(crate\) |pub )?struct \w+.*\{\s*$') { $inStruct = $true; $out += $line; continue }
        if ($inStruct -and $line -match '^\}') { $inStruct = $false; $out += $line; continue }
        if ($inStruct -and $line -match '^    (\w+):') { $out += ($line -replace '^    ', '    pub(crate) '); continue }
        if ($line -match '^(struct|enum|const|fn) ') { $out += ($line -replace '^(struct|enum|const|fn) ', 'pub(crate) $1 '); continue }
        if ($line -match '^    fn ') { $out += ($line -replace '^    fn ', '    pub(crate) fn '); continue }
        $out += $line
    }
    return $out
}

Set-Content -LiteralPath (Join-Path $target 'model.rs') -Value (@(
    '//! Audio-side data: samples, commands, voices and the engine state.',
    ''
) + $head + @('') + (Widen $model))

foreach ($entry in @(
    @{ File = 'engine.rs'; Doc = '//! The render engine: mixing, automation application and effect chains.'; Siblings = @('use super::model::*;', 'use super::dsp::*;'); Body = $engine },
    @{ File = 'dsp.rs'; Doc = '//! Free DSP helpers: oscillators, EQ, sample interpolation and WAV I/O.'; Siblings = @('use super::model::*;', 'use super::engine::*;'); Body = $dsp },
    @{ File = 'tests.rs'; Doc = '//! Tests for the engine and its DSP helpers.'; Siblings = @('use super::model::*;', 'use super::engine::*;', 'use super::dsp::*;'); Body = $tests }
)) {
    Set-Content -LiteralPath (Join-Path $target $entry.File) -Value (@(
        $entry.Doc,
        ''
    ) + $head + @('') + $entry.Siblings + @('') + (Widen $entry.Body))
}

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

# The one cross-module construction that a glob import cannot express.
$modelPath = Join-Path $target 'model.rs'
$modelLines = Get-Content -LiteralPath $modelPath
$modelLines = $modelLines | ForEach-Object {
    if ($_ -match 'AudioEngine::new\(') { $_ -replace 'AudioEngine::new\(', 'super::engine::AudioEngine::new(' } else { $_ }
}
Set-Content -LiteralPath $modelPath -Value $modelLines

Remove-Item -LiteralPath $source
Write-Host "audio split: model $($model.Count), engine $($engine.Count), dsp $($dsp.Count), tests $($tests.Count)"
