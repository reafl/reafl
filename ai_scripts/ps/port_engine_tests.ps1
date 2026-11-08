<#
.SYNOPSIS
    Ports the engine unit tests to the track model.

.DESCRIPTION
    Mechanical rewrites for `crates/reafl-core/src/audio/tests.rs`:

      * `project.instruments`            -> `project.tracks`
      * `ParameterTarget::Instrument { track_id, parameter: X }` ->
        `Track { track_id, parameter: TrackParameter::Generator(X) }`
      * `ParameterTarget::MixerChannel { channel_id, parameter: X }` ->
        `Track { track_id: channel_id, parameter: TrackParameter::Mixer(X) }`
      * `start_voice(a, b, `             -> `start_voice(a, ` (the lane and the
        track are the same index now)
      * `instrument_audible` / `lane_audible` -> `track_audible`

    Everything else is fixed by hand.

.EXAMPLE
    pwsh -File ai_scripts/ps/port_engine_tests.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$utf8 = New-Object System.Text.UTF8Encoding($false)
$path = 'crates/reafl-core/src/audio/tests.rs'
$text = [System.IO.File]::ReadAllText($path)
$original = $text

$text = $text.Replace('project.instruments', 'project.tracks')
$text = $text.Replace('instrument_audible', 'track_audible')
$text = $text.Replace('lane_audible', 'track_audible')

$text = [regex]::Replace(
    $text,
    'ParameterTarget::Instrument \{\s*track_id: ([^,\r\n]+),\s*(?:\r?\n\s*)?parameter: ([^,\r\n]+),?\s*\}',
    'ParameterTarget::Track { track_id: $1, parameter: TrackParameter::Generator($2) }')

$text = [regex]::Replace(
    $text,
    'ParameterTarget::MixerChannel \{\s*channel_id: ([^,\r\n]+),\s*(?:\r?\n\s*)?parameter: ([^,\r\n]+),?\s*\}',
    'ParameterTarget::Track { track_id: $1, parameter: TrackParameter::Mixer($2) }')

$text = [regex]::Replace($text, 'start_voice\(\s*(\d+),\s*(\d+),', 'start_voice($1,')

if ($text -ne $original) {
    [System.IO.File]::WriteAllText($path, $text, $utf8)
    'ported the engine tests'
} else {
    'nothing to do'
}
