<#
.SYNOPSIS
    Rewrites `ParameterTarget::Track { instrument_id, parameter: ... }` to the
    generator namespace.

.DESCRIPTION
    The unified target carries a `TrackParameter`, so generator controls are
    wrapped: `ParameterTarget::Track { track_id, parameter:
    TrackParameter::Generator(InstrumentParameter::X) }`. This codemod rewrites
    the remaining hand-written forms across the app crate.

.EXAMPLE
    pwsh -File ai_scripts/ps/fix_generator_targets.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$utf8 = New-Object System.Text.UTF8Encoding($false)
$pattern = 'ParameterTarget::Track \{\s*instrument_id,\s*parameter: (InstrumentParameter::\w+),?\s*\}'
$replacement = 'ParameterTarget::Track { track_id: instrument_id, parameter: TrackParameter::Generator($1) }'

$changed = 0
foreach ($file in Get-ChildItem 'crates/reafl-app/src' -Recurse -Filter *.rs) {
    $text = [System.IO.File]::ReadAllText($file.FullName)
    $updated = [regex]::Replace($text, $pattern, $replacement)
    if ($updated -ne $text) {
        [System.IO.File]::WriteAllText($file.FullName, $updated, $utf8)
        $changed++
    }
}

"rewrote targets in $changed file(s)"
