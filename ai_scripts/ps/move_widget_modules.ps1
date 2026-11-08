param(
    [string]$From = "crates/reafl-app/src",
    [string]$To = "crates/reafl-widgets/src"
)

function Move-Module {
    param([string]$Name, [string]$Use)
    git mv "$From/$Name.rs" "$To/$Name.rs" | Out-Null
    $path = Join-Path $To "$Name.rs"
    $lines = Get-Content -LiteralPath $path
    $body = $lines | Select-Object -Skip 4
    $body = $body | ForEach-Object { $_ -replace '^pub\(crate\) ', 'pub ' }
    $header = @("//! " + ($lines[0] -replace '^//! ', ''), "") + $Use.Split("`n") + @("")
    Set-Content -LiteralPath $path -Value ($header + $body)
    Write-Host "moved $Name.rs into reafl-widgets"
}

Move-Module theme @"
use eframe::egui;
use reafl_settings as settings;
"@
Move-Module icons @"
use eframe::egui;
"@
Move-Module tooltip @"
use eframe::egui;
use reafl_settings as settings;
"@
Move-Module presets @"
use std::fs;

use reafl_core::project::InsertFxSlot;
"@
