param([string]$Dir = "crates/reafl-widgets/src")

# Replaces the shotgun glob imports with the explicit ones each module needs.
$extras = @{
    'knob' = @(
        'use crate::automation_bridge::{automation_context_menu, request_automation_lane, target_is_automated, target_live_value};'
    )
    'power' = @(
        'use crate::icons::{Icon, paint_icon, power_word};',
        'use crate::tooltip::tooltip;'
    )
    'mixer_strip' = @(
        'use crate::automation_bridge::{automation_context_menu, target_is_automated, target_live_value};',
        'use crate::fader::fader;',
        'use crate::icons::{Icon, paint_icon, power_word};',
        'use crate::knob::Knob;',
        'use crate::meter::render_meter_sized;',
        'use crate::power::{PowerSwitch, power_switch_at};',
        'use crate::theme::{active_theme, lane_color};',
        'use crate::tooltip::tooltip;'
    )
    'time_edit' = @(
        'use crate::automation_bridge::automation_context_menu;'
    )
    'effect_parameters' = @(
        'use crate::automation_bridge::{automation_context_menu, target_live_value};',
        'use crate::knob::Knob;',
        'use crate::lane_slider::lane_parameter_slider;'
    )
    'automation_bridge' = @()
    'fader' = @(
        'use crate::automation_bridge::{automation_context_menu, target_is_automated, target_live_value};',
        'use crate::fader_math::{fader_gain, fader_label, fader_position};',
        'use crate::tooltip::tooltip;'
    )
    'lane_slider' = @(
        'use crate::automation_bridge::{automation_context_menu, target_live_value};'
    )
    'meter' = @(
        'use crate::theme::active_theme;'
    )
    'eq_curve' = @(
        'use crate::theme::active_theme;'
    )
}

foreach ($entry in $extras.GetEnumerator()) {
    $path = Join-Path $Dir "$($entry.Key).rs"
    $lines = Get-Content -LiteralPath $path
    $kept = $lines | Where-Object { $_ -notmatch '^use crate::\w+::\*;$' -and $_ -notmatch '^use reafl_core::project;$' }
    $insertAt = 0
    for ($i = 0; $i -lt $kept.Count; $i++) {
        if ($kept[$i] -match '^pub ') { $insertAt = $i; break }
    }
    $head = $kept[0..($insertAt - 1)]
    $rest = $kept[$insertAt..($kept.Count - 1)]
    Set-Content -LiteralPath $path -Value ($head + $entry.Value + @('') + $rest)
    Write-Host "relinked $($entry.Key).rs"
}
