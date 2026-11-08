param([string]$Path = "crates/reafl-widgets/src/tooltip.rs")

# Rewrites the module header so the tooltip settings keys live beside the code
# that reads them. The body starts after the existing doc comment and imports.
$lines = Get-Content -LiteralPath $Path
$body = $lines | Select-Object -Skip 5
$header = @(
    '//! Small hover and text helpers.',
    '',
    'use eframe::egui;',
    'use reafl_settings::settings;',
    '',
    '/// Key of the stored tooltip mode.',
    'pub const TOOLTIP_MODE_KEY: &str = "reafl.tooltip_mode";',
    '',
    '/// Key of the stored tooltip delay, in seconds.',
    'pub const TOOLTIP_DELAY_KEY: &str = "reafl.tooltip_delay";',
    ''
)
Set-Content -LiteralPath $Path -Value ($header + $body)
Write-Host "rewrote $Path"
