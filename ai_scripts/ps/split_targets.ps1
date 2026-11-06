$src = 'crates/pulseforge-core/src/project/model/targets.rs'
$model = 'crates/pulseforge-core/src/project/model'
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

$A = Doc-Start (Find-Index '^pub enum AutomationCurve \{' 0)
$B = Doc-Start (Find-Index '^impl Default for TimeSignature \{' 0)
$C = Doc-Start (Find-Index '^pub struct StepHit \{' 0)
$D = Doc-Start (Find-Index '^pub struct VoiceSettings \{' 0)
$E = Doc-Start (Find-Index '^pub enum InstrumentParameter \{' 0)
"anchors A=$A B=$B C=$C D=$D E=$E count=$($lines.Count)"

$automationPart = $lines[$A..($B - 1)]
$timePart = $lines[$B..($C - 1)]
$pianoPart = $lines[$C..($D - 1)]
$devicePart = $lines[$D..($E - 1)]
$targetPart = $lines[$E..($lines.Count - 1)]

# header of the current file keeps the module docs; reuse them for targets.rs
$header = @()
$i = 0
while ($i -lt $A -and $lines[$i].TrimStart().StartsWith('//!')) { $header += $lines[$i]; $i++ }

# --- automation: append to the existing automation module ---
$auto = Get-Content "$model/automation.rs"
$firstItem = -1
for ($i = 0; $i -lt $auto.Count; $i++) {
    if ($auto[$i] -match '^#\[derive') { $firstItem = $i; break }
}
$autoHead = $auto[0..($firstItem - 1)]
$autoBody = $auto[$firstItem..($auto.Count - 1)]
Set-Content -Path "$model/automation.rs" -Value ($autoHead + $automationPart + @('') + $autoBody) -Encoding utf8

# --- params: append the time signature impls ---
$param = Get-Content "$model/params.rs"
Set-Content -Path "$model/params.rs" -Value ($param + @('') + $timePart) -Encoding utf8

# --- piano ---
$pianoFile = @(
    '//! Notes, steps and the arpeggiator.',
    '//!',
    '//! These types describe what the sequencer plays: a [`PianoNote`] in a',
    '//! pattern, a [`StepHit`] from the step sequencer and the [`Arpeggio`]',
    '//! settings that turn held notes into a pattern.',
    '',
    'use serde::{Deserialize, Serialize};',
    '',
    'use super::params::TimeValue;',
    ''
) + $pianoPart + @('')
Set-Content -Path "$model/piano.rs" -Value $pianoFile -Encoding utf8

# --- device ---
$deviceFile = @(
    '//! Device types shared by instruments and insert effects.',
    '',
    'use serde::{Deserialize, Serialize};',
    ''
) + $devicePart + @('')
Set-Content -Path "$model/device.rs" -Value $deviceFile -Encoding utf8

# --- targets: parameter addresses only ---
$targetFile = $header + @(
    '//!',
    '//! Every automatable parameter is addressed by a [`ParameterTarget`], which',
    '//! carries the owning object and one of the parameter enums below.',
    ''
) + @('use serde::{Deserialize, Serialize};', '') + $targetPart + @('')
Set-Content -Path $src -Value $targetFile -Encoding utf8

Get-ChildItem $model | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
