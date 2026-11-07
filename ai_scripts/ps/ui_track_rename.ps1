<#
.SYNOPSIS
    Renames the UI's mixer-channel vocabulary to tracks.

.DESCRIPTION
    Insert effects and mixer controls now address a track, so the UI's
    `channel_id` variables and fields become `track_id`. Whole-identifier
    replacement only.

.EXAMPLE
    pwsh -File ai_scripts/ps/ui_track_rename.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$roots = @(
    'crates/pulseforge-app/src',
    'crates/pulseforge-widgets/src'
)

$map = [ordered]@{
    'channel_id' = 'track_id'
    'lane_count' = 'track_count'
}

$files = foreach ($root in $roots) { Get-ChildItem $root -Recurse -Filter *.rs }

$changed = 0
foreach ($file in $files) {
    $text = Get-Content $file.FullName -Raw
    $original = $text
    foreach ($pair in $map.GetEnumerator()) {
        $text = [regex]::Replace($text, "\b$($pair.Key)\b", $pair.Value)
    }
    if ($text -ne $original) {
        [System.IO.File]::WriteAllText($file.FullName, $text, (New-Object System.Text.UTF8Encoding($false)))
        $changed++
    }
}

"renamed identifiers in $changed file(s)"
