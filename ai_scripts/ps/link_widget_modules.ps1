param([string]$Dir = "crates/pulseforge-widgets/src")

$allModules = @(
    'anim', 'automation_bridge', 'context_menu', 'effect_parameters', 'eq_curve', 'fader',
    'fader_math', 'icons', 'knob', 'lane_slider', 'meter', 'mixer_strip', 'platform', 'power',
    'presets', 'theme', 'time_edit', 'tooltip'
)

# Adds glob imports of the sibling widget modules so extracted code resolves
# against its new crate, skipping the module itself.
function Add-SiblingImports {
    param([string]$Name)
    $path = Join-Path $Dir "$Name.rs"
    $lines = Get-Content -LiteralPath $path
    $insertAt = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^pub ') { $insertAt = $i; break }
    }
    $uses = @()
    foreach ($module in $allModules) {
        if ($module -ne $Name) { $uses += "use crate::$module::*;" }
    }
    $head = if ($insertAt -gt 0) { $lines[0..($insertAt - 1)] } else { @() }
    $rest = $lines[$insertAt..($lines.Count - 1)]
    Set-Content -LiteralPath $path -Value ($head + $uses + @('') + $rest)
}

# Drops the lines a cut left dangling at the end of a module.
function Trim-Tail {
    param([string]$Name, [int]$Count)
    $path = Join-Path $Dir "$Name.rs"
    $lines = Get-Content -LiteralPath $path
    Set-Content -LiteralPath $path -Value $lines[0..($lines.Count - $Count - 1)]
}

foreach ($name in @(
    'knob', 'power', 'mixer_strip', 'time_edit', 'effect_parameters', 'automation_bridge',
    'fader', 'lane_slider', 'meter', 'eq_curve'
)) {
    Add-SiblingImports $name
}

Trim-Tail 'knob' 3
Trim-Tail 'eq_curve' 3
Write-Host 'widget modules patched'
