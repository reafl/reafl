param([string]$Audio = "crates/reafl-core/src/audio")

$source = Join-Path $Audio 'engine.rs'
$folder = Join-Path $Audio 'engine'
if (-not (Test-Path $folder)) { New-Item -ItemType Directory -Force $folder | Out-Null }

# Splits a contiguous run of AudioEngine methods into a child module of `engine`.
function Cut-Methods {
    param([string]$First, [string]$Last, [string]$Target, [string]$Title)
    $lines = Get-Content -LiteralPath $source
    $starts = @{}
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^    (pub\(crate\) )?fn (\w+)') { $starts[$Matches[2]] = $i }
    }
    if (-not $starts.ContainsKey($First)) { throw "method $First not found" }
    if (-not $starts.ContainsKey($Last)) { throw "method $Last not found" }

    $startIdx = $starts[$First]
    while ($startIdx -gt 0 -and ($lines[$startIdx - 1] -match '^\s*///' -or $lines[$startIdx - 1] -match '^\s*#\[')) { $startIdx-- }
    $lastIdx = $starts[$Last]

    $boundary = $lines.Count
    for ($i = $lastIdx + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^    (pub\(crate\) )?fn ' -or $lines[$i] -match '^(impl|struct|enum|fn |const |#\[|pub )') { $boundary = $i; break }
    }
    $endIdx = $boundary - 1
    while ($endIdx -ge $startIdx -and ($lines[$endIdx].Trim() -eq '' -or $lines[$endIdx] -match '^\s*///')) { $endIdx-- }

    $chunk = $lines[$startIdx..$endIdx] | ForEach-Object {
        if ($_ -match '^    fn ') { $_ -replace '^    fn ', '    pub(crate) fn ' } else { $_ }
    }
    $path = Join-Path $folder $Target
    Set-Content -LiteralPath $path -Value (@("//! $Title", '', 'use super::*;', '') + @('impl AudioEngine {') + $chunk + @('}', ''))

    $remaining = @()
    if ($startIdx -gt 0) { $remaining += $lines[0..($startIdx - 1)] }
    if ($endIdx + 1 -lt $lines.Count) { $remaining += $lines[($endIdx + 1)..($lines.Count - 1)] }
    Set-Content -LiteralPath $source -Value $remaining
    Write-Host "$Target <- $First..$Last ($($chunk.Count) lines)"
}

Cut-Methods render_frame handle_midi_note 'render.rs' 'Rendering frames, applying commands and playing notes.'
Cut-Methods automate_effect_parameters render_hosted_instruments 'effects.rs' 'Insert chains, their parameters and hosted instruments.'
Cut-Methods update_instrument_controls release_automation 'automation.rs' 'Evaluating automation: curves, modulators and envelopes.'

# The remainder becomes the module root, with the children declared on top.
$body = Get-Content -LiteralPath $source
Set-Content -LiteralPath (Join-Path $folder 'mod.rs') -Value (@(
    '//! The render engine: state, rendering and automation.',
    '//!',
    '//! [`AudioEngine`] is one struct spread over several files: the module root',
    '//! keeps construction and the state that the routing is built from, while',
    '//! [`render`], [`effects`] and [`automation`] hold the parts that run every',
    '//! block.',
    '',
    'mod automation;',
    'mod effects;',
    'mod render;',
    ''
) + $body)
Remove-Item -LiteralPath $source
Write-Host 'engine.rs became engine/mod.rs'
