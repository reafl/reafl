param([string]$Root = "crates/pulseforge-widgets/src")

# Documents the mixer strip data model and the icon set.
$stripPath = Join-Path $Root 'mixer_strip.rs'
$lines = Get-Content -LiteralPath $stripPath

$fields = @{
    'pub channel_id: usize,' = 'Index into `project.lanes`, or the master index.'
    'pub name: Option<&''a mut String>,' = 'Channel name, edited in place when present.'
    'pub pinned: Option<&''a mut bool>,' = 'Whether the channel row is pinned.'
    'pub gain: &''a mut f32,' = 'Linear fader gain.'
    'pub pan: &''a mut f32,' = 'Pan, `-1` left to `1` right.'
    'pub eq: &''a mut ThreeBandEq,' = 'The channel''s three-band EQ gains.'
    'pub meter: f32,' = 'Peak level for the meter, `0..=1`.'
    'pub selected: bool,' = 'Whether this strip is the selected channel.'
    'pub collapsed: bool,' = 'Draws the compact form when true.'
    'pub is_master: bool,' = 'True for the master strip.'
    'pub enabled: bool,' = 'Whether the channel is switched on.'
    'pub locked: bool,' = 'Whether the channel ignores edits.'
    'pub solo: bool,' = 'Whether the channel is soloed.'
    'pub inserts: Option<&''a mut [InsertFxSlot]>,' = 'Insert slots; absent when the strip hides them.'
    'pub open_inserts: &''a std::collections::BTreeSet<(usize, usize)>,' = 'Insert slots whose plug-in window is open.'
}

$out = @()
$inStrip = $false
foreach ($line in $lines) {
    if ($line -match '^pub struct MixerStrip') {
        $out += '/// What one mixer strip is bound to.'
        $out += $line
        $inStrip = $true
        continue
    }
    if ($inStrip -and $line -match '^\}') { $inStrip = $false; $out += $line; continue }
    if ($inStrip -and $fields.ContainsKey($line)) {
        $out += ('    /// ' + $fields[$line])
        $out += $line
        continue
    }
    if ($line -match '^#\[derive\(Default\)\]$') { $out += $line; continue }
    if ($line -match '^pub struct MixerStripResult') {
        $out += '/// What a drawn strip reports back to the mixer view.'
        $out += $line
        continue
    }
    if ($line -match '^    pub changed: bool,$') { $out += '    /// True when any control on the strip changed.'; $out += $line; continue }
    if ($line -match '^    pub selected: bool,$') { $out += '    /// True when the strip asked to become the selected channel.'; $out += $line; continue }
    if ($line -match '^    pub actions: Vec<ContextAction>,$') { $out += '    /// Menu actions the view still has to apply.'; $out += $line; continue }
    if ($line -match '^pub const MIXER_FADER_HEIGHT') { $out += '/// Height of the fader and meter pair.'; $out += $line; continue }
    if ($line -match '^pub const MIXER_FADER_WIDTH') { $out += '/// Width of the fader.'; $out += $line; continue }
    if ($line -match '^pub fn mixer_strip\(') {
        $out += '/// Draws one mixer channel strip.'
        $out += '///'
        $out += '/// # Parameters'
        $out += '///'
        $out += '/// - `ui`: the parent layout; the strip allocates its own column.'
        $out += '/// - `strip`: everything the strip reads and writes.'
        $out += '///'
        $out += '/// # Returns'
        $out += '///'
        $out += '/// What changed, whether the strip was selected, and any menu actions.'
        $out += $line
        continue
    }
    $out += $line
}
Set-Content -LiteralPath $stripPath -Value $out

# Every icon variant gets a one-line description.
$iconNames = @{
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
$iconPath = Join-Path $Root 'icons.rs'
$iconLines = Get-Content -LiteralPath $iconPath
$out = @()
$inIcons = $false
foreach ($line in $iconLines) {
    if ($line -match '^pub enum Icon \{') { $inIcons = $true; $out += $line; continue }
    if ($inIcons -and $line -match '^\}') { $inIcons = $false; $out += $line; continue }
    if ($inIcons -and $iconNames.ContainsKey($line.Trim())) {
        $out += ('    /// Glyph for the ' + $iconNames[$line.Trim()] + '.')
        $out += $line
        continue
    }
    $out += $line
}
Set-Content -LiteralPath $iconPath -Value $out
Write-Host 'mixer strip and icons documented'
