param([string]$Path = "crates/pulseforge-widgets/src/icons.rs")

$names = @{
    'Rack' = 'channel rack window'
    'Roll' = 'piano roll window'
    'Mixer' = 'mixer window'
    'Routing' = 'routing window'
    'Automation' = 'automation window'
    'Samples' = 'sample browser window'
    'Midi' = 'MIDI routing window'
    'Settings' = 'settings window'
    'Render' = 'render-to-file window'
    'Debug' = 'profiler panel'
    'Undo' = 'undo'
    'Redo' = 'redo'
    'Play' = 'play the transport'
    'Stop' = 'stop the transport'
    'Loop' = 'loop the arrangement'
    'Metronome' = 'metronome toggle'
    'Power' = 'power switch'
    'Solo' = 'solo'
    'ArrowDown' = 'collapsed group'
    'ArrowRight' = 'expanded group'
}

$lines = Get-Content -LiteralPath $Path
$out = @()
$inIcons = $false
foreach ($line in $lines) {
    if ($line -match '^pub enum Icon \{') { $inIcons = $true; $out += $line; continue }
    if ($inIcons -and $line -match '^\}') { $inIcons = $false; $out += $line; continue }
    if ($inIcons) {
        $name = $line.Trim().TrimEnd(',')
        if ($names.ContainsKey($name)) {
            $out += ('    /// Glyph for the ' + $names[$name] + '.')
            $out += $line
            continue
        }
    }
    $out += $line
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'icon variants documented'
