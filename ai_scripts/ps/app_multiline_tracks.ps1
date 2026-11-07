<#
.SYNOPSIS
    Rewrites multi-line project field chains to the track list.

.DESCRIPTION
    Some calls are written across lines:

        self
            .project
            .instruments
            .get(id)

    A plain textual replace does not catch those. This codemod rewrites the
    field name in a multi-line chain, including `rustfmt` wrapped variants.

.EXAMPLE
    pwsh -File ai_scripts/ps/app_multiline_tracks.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$utf8 = New-Object System.Text.UTF8Encoding($false)
$files = Get-ChildItem 'crates/pulseforge-app/src' -Recurse -Filter *.rs
$changed = 0
foreach ($file in $files) {
    $text = [System.IO.File]::ReadAllText($file.FullName)
    $original = $text
    $text = [regex]::Replace($text, '(?m)^(\s*)\.instruments\b', '$1.tracks')
    $text = [regex]::Replace($text, '(?m)^(\s*)\.patterns\b', '$1.tracks')
    if ($text -ne $original) {
        [System.IO.File]::WriteAllText($file.FullName, $text, $utf8)
        $changed++
    }
}

"rewrote chains in $changed file(s)"
