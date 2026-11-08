param(
    [string]$Source = "crates/reafl-app/src/lib.rs",
    [string]$Root = "crates/reafl-app/src"
)

# Splits a contiguous run of `ReaflApp` methods out of the app file and into
# its own module, wrapped in its own `impl ReaflApp` block.
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
    $block = @('impl ReaflApp {') + $chunk + @('}', '')
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

Cut-Range new preview_sample 'app_setup.rs' 'Constructing the editor and applying its start-up settings.'
Cut-Range egui_key perform 'input.rs' 'Keyboard bindings: chord lookup and the command dispatcher.'
Cut-Range peaks_for draw_sample_waveform 'samples.rs' 'Loading samples, waveform peaks and audio clips.'
Cut-Range selected_note_count instrument_window_title 'selection.rs' 'What is selected and how the selection grows.'
Cut-Range selection_modifiers select_channel 'selection.rs' '' -Append
Cut-Range metadata_for poll_last_tweaked 'hosting.rs' 'Hosting VST3/CLAP instances and their editor windows.'
Cut-Range create_automation_for_target create_plugin_automation 'automation_actions.rs' 'Creating automation lanes for new targets.'
Cut-Range resize_editor_to sync_editor_window 'hosting.rs' '' -Append
Cut-Range render_hosted_plugin render_external_plugin_wrapper 'views/plugins.rs' '' -Append
Cut-Range instrument_color toggle_instrument_switch 'instruments.rs' 'Instrument naming, colours and power switches.'
Cut-Range sender playhead_step 'transport.rs' 'Project syncing, history, undo and the playhead.'
Cut-Range place_source_clip load_audio_clip_samples 'clips.rs' 'Placing, slicing and decoding arrangement clips.'
Cut-Range capture_plugin_states effect_slot_path 'hosting.rs' '' -Append
Cut-Range save_project load_instrument_samples 'project_io.rs' 'Saving, loading and the background-task results.'
Cut-Range query_midi_ports connect_midi 'project_io.rs' '' -Append
Cut-Range add_formula_automation add_formula_automation 'views/automation.rs' '' -Append
Cut-Range live_automated_values automated_display_value 'automation_display.rs' 'The values a lane currently drives, for the widgets.'
