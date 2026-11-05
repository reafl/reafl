param([string]$Path = "crates/pulseforge-widgets/src/tooltip.rs")

# Rewrites the module header so the tooltip settings keys live beside the code
# that reads them. The body starts after the existing doc comment and imports.
$lines = Get-Content -LiteralPath $Path
$body = $lines | Select-Object -Skip 5
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
    ''
)
Set-Content -LiteralPath $Path -Value ($header + $body)
Write-Host "rewrote $Path"
