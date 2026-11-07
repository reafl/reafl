<#
.SYNOPSIS
    Renames the app's lane accesses to the unified track list.

.DESCRIPTION
    `Project::lanes` is gone; every row is a `Track` whose fields keep the old
    lane names. This codemod rewrites the remaining `*.lanes` accesses in the
    app and widget crates to `*.tracks`; anything that still needs a generator
    is fixed by hand afterwards.

.EXAMPLE
    pwsh -File ai_scripts/ps/app_lane_to_track.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$roots = @(
    'crates/pulseforge-app/src',
    'crates/pulseforge-widgets/src'
)

$files = foreach ($root in $roots) { Get-ChildItem $root -Recurse -Filter *.rs }
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
