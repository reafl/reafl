param(
    [string]$Source = "crates/pulseforge-app/src/lib.rs",
    [string]$Root = "crates/pulseforge-app/src"
)

# Splits a contiguous run of `PulseForgeApp` methods out of the monolith and into
# its own module, wrapped in its own `impl PulseForgeApp` block.
function Cut-Range {
    param(
        [string]$First,
        [string]$Last,
        [string]$Target,
        [string]$Title,
        [switch]$Append
    )
    $lines = Get-Content -LiteralPath $Source
    $starts = @{}
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^    (pub )?fn (\w+)') { $starts[$Matches[2]] = $i }
    }
    if (-not $starts.ContainsKey($First)) { throw "method $First not found" }
    if (-not $starts.ContainsKey($Last)) { throw "method $Last not found" }

    $startIdx = $starts[$First]
    while ($startIdx -gt 0 -and ($lines[$startIdx - 1] -match '^\s*///' -or $lines[$startIdx - 1] -match '^\s*#\[')) {
        $startIdx--
    }
    $lastIdx = $starts[$Last]

    $boundary = $lines.Count
    for ($i = $lastIdx + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^    (pub )?fn ' -or
            $lines[$i] -match '^(impl|struct|enum|fn |const |static |#\[cfg|\})') {
            $boundary = $i
            break
        }
    }
    $endIdx = $boundary - 1
    while ($endIdx -ge $startIdx -and ($lines[$endIdx].Trim() -eq '' -or $lines[$endIdx] -match '^\s*///')) {
        $endIdx--
    }

    $chunk = $lines[$startIdx..$endIdx] | ForEach-Object { $_ -replace '^    (pub )?fn ', '    pub(crate) fn ' }
    $path = Join-Path $Root $Target
    $dir = Split-Path -Parent $path
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
    $block = @('impl PulseForgeApp {') + $chunk + @('}', '')
    if ($Append -and (Test-Path $path)) {
        Add-Content -LiteralPath $path -Value $block
    }
    else {
        Set-Content -LiteralPath $path -Value (@("//! $Title", '', 'use crate::*;', '') + $block)
    }

    $remaining = @()
    if ($startIdx -gt 0) { $remaining += $lines[0..($startIdx - 1)] }
    if ($endIdx + 1 -lt $lines.Count) { $remaining += $lines[($endIdx + 1)..($lines.Count - 1)] }
    Set-Content -LiteralPath $Source -Value $remaining
    Write-Host "$Target <- $First..$Last ($($chunk.Count) lines)"
}

Cut-Range render_toolbar render_clip_picker 'views/toolbar.rs' 'The toolbar and the clip picker beside the arrangement.'
Cut-Range render_arrangement render_timeline 'views/arrangement.rs' 'The arrangement: lanes, clips, automation rows and the timeline.'
Cut-Range render_channel_rack set_pattern_hit_for 'views/rack.rs' 'The channel rack and its step sequencer.'
Cut-Range render_piano_roll join_notes 'views/piano.rs' 'The piano roll, its gestures and note editing.'
Cut-Range plugin_window_header render_plugin_selector 'views/plugins.rs' 'Instrument and effect plug-in windows.'
Cut-Range render_automation_window render_sample_clip_window 'views/automation.rs' 'The automation, export and sample-clip windows.'
Cut-Range add_mixer_channel render_mixer_inspector 'views/mixer.rs' 'The mixer strips and the channel inspector.'
Cut-Range apply_context_action make_clip_pattern_unique 'actions.rs' 'Context-menu and clip actions.'
Cut-Range meter_level selection_label 'views/panels.rs' 'Routing, MIDI, footer and status panels.'
Cut-Range render_settings_window load_sample_into_selected 'views/settings.rs' 'The settings window and its tabs.'
