<#
.SYNOPSIS
    Renames the audio engine's instrument/lane vocabulary to tracks.

.DESCRIPTION
    The project model now has one `Track` per arrangement row, mixer strip and
    rack slot, so the engine's separate `instrument_id` and `mixer_lane` fields
    are the same index. This codemod renames every occurrence in the engine:

        instrument_id -> track_id
        mixer_lane    -> track_id
        lane_id       -> track_id
        lane_count    -> track_count
        lane_inputs   -> track_inputs
        lane_peak_pre -> track_peak_pre
        lane_peak_post -> track_peak_post

    Only whole identifiers are touched, so `mixer_lane` is not affected by the
    `lane_id` rule and vice versa.

.EXAMPLE
    pwsh -File ai_scripts/ps/track_rename.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$roots = @(
    'crates/reafl-core/src/audio',
    'crates/reafl-core/src/hosted.rs'
)

$map = [ordered]@{
    'instrument_id'  = 'track_id'
    'mixer_lane'     = 'track_id'
    'lane_id'        = 'track_id'
    'lane_count'     = 'track_count'
    'lane_inputs'    = 'track_inputs'
    'lane_peak_pre'  = 'track_peak_pre'
    'lane_peak_post' = 'track_peak_post'
}

$files = foreach ($root in $roots) {
    if (Test-Path $root -PathType Container) {
        Get-ChildItem $root -Recurse -Filter *.rs
    } elseif (Test-Path $root) {
        Get-Item $root
    }
}

$changed = 0
foreach ($file in $files) {
    $text = Get-Content $file.FullName -Raw
    $original = $text
    foreach ($pair in $map.GetEnumerator()) {
        $pattern = "\b$($pair.Key)\b"
        $text = [regex]::Replace($text, $pattern, $pair.Value)
    }
    if ($text -ne $original) {
        [System.IO.File]::WriteAllText($file.FullName, $text, (New-Object System.Text.UTF8Encoding($false)))
        $changed++
    }
}

"renamed identifiers in $changed file(s)"
