param([string]$Dir = "crates/reafl-widgets/src")

function Insert-BeforeFirstItem {
    param([string]$Name, [string[]]$Lines)
    $path = Join-Path $Dir "$Name.rs"
    $content = Get-Content -LiteralPath $path
    $insertAt = 0
    for ($i = 0; $i -lt $content.Count; $i++) {
        if ($content[$i] -match '^pub ') { $insertAt = $i; break }
    }
    $head = $content[0..($insertAt - 1)]
    $rest = $content[$insertAt..($content.Count - 1)]
    Set-Content -LiteralPath $path -Value ($head + $Lines + @('') + $rest)
}

Insert-BeforeFirstItem 'fader' @('use reafl_core::project::ParameterTarget;')
Insert-BeforeFirstItem 'time_edit' @('use reafl_core::project::ParameterTarget;')
Insert-BeforeFirstItem 'mixer_strip' @(
    'use reafl_core::project::{InsertFxSlot, ThreeBandEq};',
    '',
    'use crate::eq_curve::draw_eq_curve;',
    'use crate::fader_math::fader_label;',
    'use crate::power::power_switch;'
)

# The knob's builder methods are called from other windows now.
$knobPath = Join-Path $Dir 'knob.rs'
$knob = Get-Content -LiteralPath $knobPath
$knob = $knob | ForEach-Object { $_ -replace '^    fn ', '    pub fn ' }
Set-Content -LiteralPath $knobPath -Value $knob
Write-Host 'knob methods made public'
