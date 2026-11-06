param(
    [string]$Source = "crates/pulseforge-core/src/audio/engine/render.rs",
    [string]$Root = "crates/pulseforge-core/src/audio/engine"
)

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
        if ($lines[$i] -match '^    (pub\(crate\) )?fn ' -or $lines[$i] -match '^(impl|struct|enum|fn |const |#\[|pub |\})') { $boundary = $i; break }
    }
    $endIdx = $boundary - 1
    while ($endIdx -ge $startIdx -and ($lines[$endIdx].Trim() -eq '' -or $lines[$endIdx] -match '^\s*///')) { $endIdx-- }

    $chunk = $lines[$startIdx..$endIdx]
    Set-Content -LiteralPath (Join-Path $Root $Target) -Value (@("//! $Title", '', 'use super::*;', '') + @('impl AudioEngine {') + $chunk + @('}', ''))

    $remaining = @()
    if ($startIdx -gt 0) { $remaining += $lines[0..($startIdx - 1)] }
    if ($endIdx + 1 -lt $lines.Count) { $remaining += $lines[($endIdx + 1)..($lines.Count - 1)] }
    Set-Content -LiteralPath $Source -Value $remaining
    Write-Host "$Target <- $First..$Last ($($chunk.Count) lines)"
}

Cut-Methods trigger_step handle_midi_note 'voices.rs' 'Step triggering, arpeggios, voice allocation and MIDI note handling.'
Cut-Methods apply_pending apply_project 'commands.rs' 'Draining the command queue and adopting a new project.'
