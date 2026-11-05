param([string]$Dir = "crates/pulseforge-widgets/src")

$headers = @{
    'knob' = @(
        'use eframe::egui;',
        'use pulseforge_core::project::ParameterTarget;',
        '',
        'use crate::automation_bridge::{automation_context_menu, request_automation_lane, target_is_automated, target_live_value};',
        'use crate::platform;'
    )
    'power' = @(
        'use eframe::egui;',
        '',
        'use crate::context_menu::PowerIntent;',
        'use crate::icons::{Icon, paint_icon, power_word};',
        'use crate::tooltip::tooltip;'
    )
    'mixer_strip' = @(
        'use eframe::egui;',
        'use pulseforge_core::project::{MixerParameter, ParameterTarget};',
        'use pulseforge_settings::settings;',
        '',
        'use crate::automation_bridge::{automation_context_menu, target_is_automated, target_live_value};',
        'use crate::context_menu::{ContextAction, ContextMenu, MenuItem};',
        'use crate::fader::fader;',
        'use crate::icons::{Icon, paint_icon, power_word};',
        'use crate::knob::Knob;',
        'use crate::meter::render_meter_sized;',
        'use crate::power::{PowerSwitch, power_switch_at};',
        'use crate::theme::{active_theme, lane_color};',
        'use crate::tooltip::tooltip;'
    )
    'time_edit' = @(
        'use eframe::egui;',
        'use pulseforge_core::project;',
        '',
        'use crate::automation_bridge::automation_context_menu;'
    )
    'effect_parameters' = @(
        'use eframe::egui;',
        'use pulseforge_core::project::{EffectParameter, InsertFxSlot, ParameterTarget};',
        '',
        'use crate::automation_bridge::{automation_context_menu, target_live_value};',
        'use crate::knob::Knob;',
        'use crate::lane_slider::lane_parameter_slider;'
    )
    'automation_bridge' = @(
        'use eframe::egui;',
        'use pulseforge_core::project::ParameterTarget;'
    )
    'fader' = @(
        'use eframe::egui;',
        '',
        'use crate::automation_bridge::{automation_context_menu, target_is_automated, target_live_value};',
        'use crate::fader_math::{fader_gain, fader_label, fader_position};',
        'use crate::tooltip::tooltip;'
    )
    'lane_slider' = @(
        'use eframe::egui;',
        'use pulseforge_core::project::ParameterTarget;',
        '',
        'use crate::automation_bridge::{automation_context_menu, target_live_value};'
    )
    'meter' = @(
        'use eframe::egui;',
        '',
        'use crate::theme::active_theme;'
    )
    'eq_curve' = @(
        'use eframe::egui;',
        'use pulseforge_core::project::ThreeBandEq;',
        '',
        'use crate::theme::active_theme;'
    )
}

foreach ($entry in $headers.GetEnumerator()) {
    $path = Join-Path $Dir "$($entry.Key).rs"
    $lines = Get-Content -LiteralPath $path
    $insertAt = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^pub ') { $insertAt = $i; break }
    }
    $body = $lines[$insertAt..($lines.Count - 1)]
    $doc = $lines[0]
    Set-Content -LiteralPath $path -Value (@($doc, '') + $entry.Value + @('') + $body)
    Write-Host "reset header of $($entry.Key).rs"
}

# tooltip.rs had duplicated constants from an earlier pass; rebuild it whole.
$tooltipPath = Join-Path $Dir 'tooltip.rs'
$tooltip = Get-Content -LiteralPath $tooltipPath
$start = 0
for ($i = 0; $i -lt $tooltip.Count; $i++) {
    if ($tooltip[$i] -match '^pub fn tooltip') { $start = $i; break }
}
$body = if ($start -gt 0) { $tooltip[$start..($tooltip.Count - 1)] } else { $tooltip }
$header = @(
    '//! Small hover and text helpers.',
    '',
    'use eframe::egui;',
    'use pulseforge_settings::settings;',
    '',
    '/// Key of the stored tooltip mode.',
    'pub const TOOLTIP_MODE_KEY: &str = "pulseforge.tooltip_mode";',
    '',
    '/// Key of the stored tooltip delay, in seconds.',
    'pub const TOOLTIP_DELAY_KEY: &str = "pulseforge.tooltip_delay";',
    '',
    '/// Key the hovered widget text is published under for the status bar.',
    'pub const HOVER_TEXT_KEY: &str = "pulseforge.hover_text";',
    ''
)
Set-Content -LiteralPath $tooltipPath -Value ($header + $body)
Write-Host 'rebuilt tooltip.rs'
