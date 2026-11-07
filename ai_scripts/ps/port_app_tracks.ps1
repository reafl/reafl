<#
.SYNOPSIS
    Ports the remaining app references from instruments/lanes to tracks.

.DESCRIPTION
    Mechanical rewrites for the files that still speak the old vocabulary:

        self.project.instruments.len()  -> self.project.tracks.len()
        self.project.instruments        -> self.project.tracks
        ParameterTarget::Instrument     -> ParameterTarget::Track
        AutomationSource::MidiEnvelope { instrument_id -> track_id
        Selection::Clip { lane -> track
        clip.source (Audio)            -> clip.audio_path

    Everything structural is fixed by hand afterwards; this only removes the
    noise.

.EXAMPLE
    pwsh -File ai_scripts/ps/port_app_tracks.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$utf8 = New-Object System.Text.UTF8Encoding($false)
$roots = @('crates/pulseforge-app/src')

$map = [ordered]@{
    'ParameterTarget::Instrument'                      = 'ParameterTarget::Track'
    'Selection::Clip { lane, clip }'                    = 'Selection::Clip { track: lane, clip }'
    'Selection::Clip { lane,'                           = 'Selection::Clip { track: lane,'
    'self.project.instruments.len()'                    = 'self.project.tracks.len()'
    'self.project.instruments'                          = 'self.project.tracks'
    'self.project.patterns.len()'                       = 'self.project.tracks.len()'
    'self.project.patterns'                             = 'self.project.tracks'
    'instrument_id: instrument'                         = 'track_id: instrument'
    'MidiEnvelope { instrument_id, .. }'                = 'MidiEnvelope { track_id, .. }'
    'MidiEnvelope { instrument_id'                      = 'MidiEnvelope { track_id'
}

$changed = 0
foreach ($root in $roots) {
    foreach ($file in Get-ChildItem $root -Recurse -Filter *.rs) {
        $text = [System.IO.File]::ReadAllText($file.FullName)
        $original = $text
        foreach ($pair in $map.GetEnumerator()) {
            $text = $text.Replace($pair.Key, $pair.Value)
        }
        if ($text -ne $original) {
            [System.IO.File]::WriteAllText($file.FullName, $text, $utf8)
            $changed++
        }
    }
}

"ported $changed file(s)"
