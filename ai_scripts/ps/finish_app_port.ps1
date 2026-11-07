<#
.SYNOPSIS
    Finishes the mechanical part of the track migration in the app crate.

.DESCRIPTION
    Applies the last batch of textual rewrites that do not change structure:

      * `TrackParameter::Gain`                      -> Generator(Gain)
      * `Selection::Note { pattern, instrument }`   -> { track, clip }
      * `ParameterTarget::MixerChannel { track_id, parameter }` -> Track { track_id,
        parameter: Mixer(..) }
      * `ParameterTarget::Clip { lane_id, .. }`     -> { track_id, .. }
      * `AutomationSource::Peak { track_id, .. }`   -> { channel_id, .. }
      * `Project::add_lane()`                       -> add_track()

    Structure changes are fixed by hand afterwards.

.EXAMPLE
    pwsh -File ai_scripts/ps/finish_app_port.ps1
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

    $text = $text.Replace(
        'AutomationSource::Peak { track_id, .. }',
        'AutomationSource::Peak { channel_id: track_id, .. }')
    $text = $text.Replace('self.project.add_lane()', 'self.project.add_track()')
    $text = $text.Replace('ParameterTarget::Clip { lane_id,', 'ParameterTarget::Clip { track_id: lane_id,')

    $text = [regex]::Replace(
        $text,
        'Selection::Note \{\s*pattern: ([^,\r\n]+),\s*(?:\r?\n\s*)?instrument: ([^,\r\n]+),',
        'Selection::Note { track: $2, clip: $1,')

    $text = [regex]::Replace(
        $text,
        'ParameterTarget::MixerChannel \{\s*track_id: ([^,\r\n]+),\s*(?:\r?\n\s*)?parameter: ([^,\r\n]+),?\s*\}',
        'ParameterTarget::Track { track_id: $1, parameter: TrackParameter::Mixer($2) }')

    $text = [regex]::Replace(
        $text,
        'MidiEnvelope \{\s*instrument_id,',
        'MidiEnvelope { track_id: instrument_id,')

    if ($text -ne $original) {
        [System.IO.File]::WriteAllText($file.FullName, $text, $utf8)
        $changed++
    }
}

"finished the mechanical port in $changed file(s)"
