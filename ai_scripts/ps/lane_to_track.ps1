<#
.SYNOPSIS
    Renames the project's lane list to the unified track list in the engine.

.DESCRIPTION
    `Project::lanes` is gone; every lane is a `Track`. This codemod rewrites the
    remaining `*.lanes` accesses in the audio crate to `*.tracks`, which is safe
    because no other struct in that crate has a `lanes` field.

.EXAMPLE
    pwsh -File ai_scripts/ps/lane_to_track.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$files = Get-ChildItem 'crates/reafl-core/src/audio' -Recurse -Filter *.rs
$changed = 0
foreach ($file in $files) {
    $text = Get-Content $file.FullName -Raw
    $original = $text
    $text = [regex]::Replace($text, '\.lanes\b', '.tracks')
    if ($text -ne $original) {
        [System.IO.File]::WriteAllText($file.FullName, $text, (New-Object System.Text.UTF8Encoding($false)))
        $changed++
    }
}

"rewrote lane accesses in $changed file(s)"
