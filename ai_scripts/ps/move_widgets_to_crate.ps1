param(
    [string]$Source = "crates/reafl-app/src/lib.rs",
    [string]$TargetDir = "crates/reafl-widgets/src"
)

$WidgetImports = @(
    'use eframe::egui;',
    'use reafl_core::project::{EffectParameter, InsertFxSlot, ParameterTarget, ThreeBandEq};',
    'use reafl_settings::settings;',
    '',
    'use crate::context_menu::{ContextAction, ContextMenu, MenuItem, PowerIntent};',
    'use crate::fader_math::{fader_gain, fader_label, fader_position};',
    'use crate::icons::{Icon, icon_button, icon_button_toggle, paint_icon, power_word};',
    'use crate::platform;',
    'use crate::theme::active_theme;',
    'use crate::tooltip::tooltip;'
)

# Moves a contiguous block of top-level items out of the app monolith and into a
# module of the widgets crate.
function Cut-Widget {
    param(
        [string]$File,
        [int]$StartLine,
        [int]$EndLine,
        [string]$Name,
        [string]$Title
    )
    $lines = Get-Content -LiteralPath $File
    $chunk = $lines[($StartLine - 1)..($EndLine - 1)]
    $body = $chunk | ForEach-Object {
        $_ -replace '^(fn |struct |enum |const |static )', 'pub $1'
    }
    $header = @("//! $Title", '') + $WidgetImports + @('')
    Set-Content -LiteralPath (Join-Path $TargetDir "$Name.rs") -Value ($header + $body)
    $before = $lines[0..($StartLine - 2)]
    $after = if ($EndLine -lt $lines.Count) { $lines[$EndLine..($lines.Count - 1)] } else { @() }
    Set-Content -LiteralPath $File -Value ($before + $after)
    Write-Host "extracted widgets/$Name.rs ($($chunk.Count) lines)"
}

Cut-Widget $Source 11484 11517 eq_curve "The three-band EQ response curve."
Cut-Widget $Source 11456 11483 meter "The segmented level meter."
Cut-Widget $Source 11436 11455 lane_slider "A slider that can become an automation target."
Cut-Widget $Source 11372 11435 fader "The vertical dB fader."
Cut-Widget $Source 11313 11371 automation_bridge "The widget side of automation: targets, live values and menus."
Cut-Widget $Source 11143 11312 effect_parameters "Controls for one insert slot's parameters."
Cut-Widget $Source 11092 11142 time_edit "The time-value editor with its unit chooser."
Cut-Widget $Source 10637 11091 mixer_strip "One mixer channel strip."
Cut-Widget $Source 10542 10636 power "The power switch and its modifier intents."
Cut-Widget $Source 10317 10541 knob "The rotary knob and its automation menu."
