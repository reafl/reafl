$src = 'crates/pulseforge-core/src/project/behaviour.rs'
$dir = 'crates/pulseforge-core/src/project/behaviour'
if (Test-Path $dir) { throw "already exists: $dir" }
New-Item -ItemType Directory -Path $dir | Out-Null
$lines = Get-Content $src

function Find-Index($pattern, $from) {
    for ($i = $from; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match $pattern) { return $i }
    }
    throw "not found: $pattern"
}
function Doc-Start($index) {
    $i = $index
    while ($i -gt 0 -and ($lines[$i - 1].TrimStart().StartsWith('///') -or $lines[$i - 1].TrimStart().StartsWith('#['))) { $i-- }
    return $i
}

$A = Find-Index '^impl Project \{$' 0
$B = Doc-Start (Find-Index '^    pub fn has_automation' 0)
$C = Doc-Start (Find-Index '^    pub fn add_lane' 0)
$E = Doc-Start (Find-Index '^    /// Adds or removes the step-sequencer note' 0)
$F = Doc-Start (Find-Index '^pub fn finite_clamp' 0)
"anchors A=$A B=$B C=$C E=$E F=$F count=$($lines.Count)"

$normalize = $lines[($A + 1)..($B - 1)]
$automation = $lines[$B..($C - 1)]
$lanes = $lines[$C..($E - 1)]
$steps = $lines[$E..($F - 1)]
$free = $lines[$F..($lines.Count - 1)]
# drop the trailing `}` that closed the original impl
while ($free.Count -gt 0 -and $free[$free.Count - 1].Trim() -eq '') { $free = $free[0..($free.Count - 2)] }
if ($free[$free.Count - 1].Trim() -eq '}') { $free = $free[0..($free.Count - 2)] }

function Wrap($title, $body) {
    return @('//! ' + $title, '', 'use super::*;', '', 'impl Project {') + $body + @('}', '')
}

Set-Content -Path "$dir/normalize.rs" -Value (Wrap 'Range clamping for every numeric field of a project.' $normalize) -Encoding utf8
Set-Content -Path "$dir/automation.rs" -Value (Wrap 'Reading automation values and base parameter values.' $automation) -Encoding utf8
Set-Content -Path "$dir/lanes.rs" -Value (Wrap 'Lane and automation ordering, solo helpers and normalisation of inserts.' $lanes) -Encoding utf8
Set-Content -Path "$dir/steps.rs" -Value (Wrap 'Step-sequencer and pattern edits.' $steps) -Encoding utf8

$main = @(
    '//! What a [`Project`] can do: normalising, automation lookup, lane edits.',
    '//!',
    '//! The methods are grouped by topic so each file stays readable:',
    '//! [`normalize`] clamps every numeric field, [`automation`] reads automation',
    '//! and parameter values, [`lanes`] reorders and normalises channels, and',
    '//! [`steps`] edits the step sequencer and the pattern list.',
    '',
    'mod automation;',
    'mod lanes;',
    'mod normalize;',
    'mod steps;',
    '',
    'use super::*;',
    ''
) + $free + @('')
Set-Content -Path "$dir/mod.rs" -Value $main -Encoding utf8

Remove-Item $src
Get-ChildItem $dir | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
