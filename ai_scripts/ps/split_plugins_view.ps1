param(
    [string]$Source = "crates/reafl-app/src/views/plugins.rs",
    [string]$Root = "crates/reafl-app/src/views"
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
    Set-Content -LiteralPath (Join-Path $Root $Target) -Value (@("//! $Title", '', 'use crate::*;', '') + @('impl ReaflApp {') + $chunk + @('}', ''))

    $remaining = @()
    if ($startIdx -gt 0) { $remaining += $lines[0..($startIdx - 1)] }
    if ($endIdx + 1 -lt $lines.Count) { $remaining += $lines[($endIdx + 1)..($lines.Count - 1)] }
    Set-Content -LiteralPath $Source -Value $remaining
    Write-Host "$Target <- $First..$Last ($($chunk.Count) lines)"
}

Cut-Methods render_hosted_plugin render_external_plugin_wrapper 'plugin_hosted.rs' 'Wrappers for hosted VST3/CLAP editors inside egui.'
Cut-Methods render_effect_plugin render_plugin_selector 'plugin_effect.rs' 'Insert plug-in windows and the plug-in picker.'
Cut-Methods render_instrument_plugin render_instrument_plugin 'plugin_instrument.rs' 'The instrument plug-in window and synth controls.'
