param([string]$Dir = "crates/reafl-widgets/src")

# Adds a missing `project` import and the automation keys, and moves the last
# document comment a cut left behind onto the item it describes.
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

Insert-BeforeFirstItem 'effect_parameters' @('use reafl_core::project;')
Insert-BeforeFirstItem 'mixer_strip' @('use reafl_core::project;')
Insert-BeforeFirstItem 'automation_bridge' @(
    '/// Key of the automated target list published every frame.',
    'pub const AUTOMATED_TARGETS_KEY: &str = "reafl.automated_targets";',
    '',
    '/// Key of the automated live value list published every frame.',
    'pub const AUTOMATED_VALUES_KEY: &str = "reafl.automated_values";'
)

# Drop the orphaned doc comment at the end of knob.rs and put it on PowerSwitch.
$knobPath = Join-Path $Dir 'knob.rs'
$knob = Get-Content -LiteralPath $knobPath
$count = 0
for ($i = $knob.Count - 1; $i -ge 0; $i--) {
    if ($knob[$i] -match '^\s*(///|//!)') { $count++ } else { break }
}
if ($count -gt 0) {
    $doc = $knob[($knob.Count - $count)..($knob.Count - 1)]
    Set-Content -LiteralPath $knobPath -Value $knob[0..($knob.Count - $count - 1)]
    Insert-BeforeFirstItem 'power' $doc
    Write-Host "moved $count doc line(s) onto PowerSwitch"
}
