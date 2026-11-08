param([string]$Dir = "crates/reafl-widgets/src")

# Moves the doc comment that a cut left at the end of one module onto the first
# item of the next module.
function Move-TailDoc {
    param([string]$From, [string]$To)
    $fromPath = Join-Path $Dir $From
    $toPath = Join-Path $Dir $To
    $lines = Get-Content -LiteralPath $fromPath
    $count = 0
    for ($i = $lines.Count - 1; $i -ge 0; $i--) {
        if ($lines[$i] -match '^\s*(///|//!)') { $count++ } else { break }
    }
    if ($count -eq 0) {
        Write-Host "$From has no trailing doc comment"
        return
    }
    $tail = $lines[($lines.Count - $count)..($lines.Count - 1)]
    Set-Content -LiteralPath $fromPath -Value $lines[0..($lines.Count - $count - 1)]

    $target = Get-Content -LiteralPath $toPath
    $insertAt = 0
    for ($i = 0; $i -lt $target.Count; $i++) {
        if ($target[$i] -match '^pub ') { $insertAt = $i; break }
    }
    $head = if ($insertAt -gt 0) { $target[0..($insertAt - 1)] } else { @() }
    $rest = $target[$insertAt..($target.Count - 1)]
    Set-Content -LiteralPath $toPath -Value ($head + $tail + $rest)
    Write-Host "moved $count doc line(s) from $From to $To"
}

# Adds the shared imports the extracted widget code expects.
function Add-Imports {
    param([string]$Name, [string[]]$Uses)
    $path = Join-Path $Dir $Name
    $lines = Get-Content -LiteralPath $path
    $insertAt = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^pub ') { $insertAt = $i; break }
    }
    $head = if ($insertAt -gt 0) { $lines[0..($insertAt - 1)] } else { @() }
    $rest = $lines[$insertAt..($lines.Count - 1)]
    Set-Content -LiteralPath $path -Value ($head + $Uses + $rest)
}

Move-TailDoc knob.rs power.rs
Move-TailDoc power.rs mixer_strip.rs
Move-TailDoc mixer_strip.rs time_edit.rs
Move-TailDoc time_edit.rs effect_parameters.rs
Move-TailDoc effect_parameters.rs automation_bridge.rs
Move-TailDoc automation_bridge.rs fader.rs
Move-TailDoc fader.rs lane_slider.rs
Move-TailDoc lane_slider.rs meter.rs
Move-TailDoc meter.rs eq_curve.rs

$shared = @('use reafl_core::project;', 'use crate::theme::lane_color;')
foreach ($name in @(
    'knob.rs', 'power.rs', 'mixer_strip.rs', 'time_edit.rs', 'effect_parameters.rs',
    'automation_bridge.rs', 'fader.rs', 'lane_slider.rs', 'meter.rs', 'eq_curve.rs'
)) {
    Add-Imports $name $shared
}
