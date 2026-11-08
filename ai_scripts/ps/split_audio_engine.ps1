param(
    [string]$Source = "crates/reafl-core/src/audio/engine.rs",
    [string]$Root = "crates/reafl-core/src/audio/engine"
)

if (-not (Test-Path $Root)) { New-Item -ItemType Directory -Force $Root | Out-Null }

# Splits a contiguous run of AudioEngine methods into its own module.
function Cut-Methods {
    param([string]$First, [string]$Last, [string]$Target, [string]$Title)
    $lines = Get-Content -LiteralPath $Source
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
    $path = Join-Path $Root $Target
    Set-Content -LiteralPath $path -Value (@("//! $Title", '', 'use super::*;', '') + @('impl AudioEngine {') + $chunk + @('}', ''))

    $remaining = @()
    if ($startIdx -gt 0) { $remaining += $lines[0..($startIdx - 1)] }
    if ($endIdx + 1 -lt $lines.Count) { $remaining += $lines[($endIdx + 1)..($lines.Count - 1)] }
    Set-Content -LiteralPath $Source -Value $remaining
    Write-Host "$Target <- $First..$Last ($($chunk.Count) lines)"
}

Cut-Methods render_frame handle_midi_note 'engine/render.rs' 'Rendering frames, applying commands and playing notes.'
Cut-Methods automate_effect_parameters render_hosted_instruments 'engine/effects.rs' 'Insert chains, their parameters and hosted instruments.'
Cut-Methods update_instrument_controls release_automation 'engine/automation.rs' 'Evaluating automation: curves, modulators and envelopes.'
