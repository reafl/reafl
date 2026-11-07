<#
.SYNOPSIS
    Second pass over the engine tests: generator kinds instead of synth flags.

.DESCRIPTION
    The instrument flags `synth_kind` and `synth_enabled` are gone; a track's
    generator owns a `GeneratorKind`. This codemod rewrites the test setup calls
    to the `use_synth` helper defined in the test module and drops the
    `synth_enabled` assignments, which the kind now implies.

.EXAMPLE
    pwsh -File ai_scripts/ps/port_engine_tests2.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$utf8 = New-Object System.Text.UTF8Encoding($false)
$path = 'crates/pulseforge-core/src/audio/tests.rs'
$text = [System.IO.File]::ReadAllText($path)
$original = $text

$text = [regex]::Replace(
    $text,
    '(\w+)\.tracks\[([^\]]+)\]\.synth_kind = ([A-Za-z0-9_:]+);',
    'use_synth(&mut $1.tracks[$2], $3);')

$text = [regex]::Replace(
    $text,
    '(?m)^\s*\w+\.tracks\[[^\]]+\]\.synth_enabled = (?:true|false);\r?\n',
    '')

if ($text -ne $original) {
    [System.IO.File]::WriteAllText($path, $text, $utf8)
    'rewrote generator setup'
} else {
    'nothing to do'
}
