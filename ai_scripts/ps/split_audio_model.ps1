$src = 'crates/reafl-core/src/audio/model.rs'
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

$A = Doc-Start (Find-Index '^pub struct SampleData \{' 0)
$B = Doc-Start (Find-Index '^pub enum AudioCommand \{' 0)
$C = Doc-Start (Find-Index '^pub struct AudioOutput \{' 0)
$D = Doc-Start (Find-Index '^pub\(crate\) struct Voice \{' 0)
"anchors A=$A B=$B C=$C D=$D count=$($lines.Count)"

$sample = $lines[$A..($B - 1)]
$output = $lines[$C..($D - 1)]

$sampleFile = @(
    '//! Decoded samples shared between the UI thread and the audio thread.',
    '',
    'use std::f32::consts::TAU;',
    '',
    'use crate::project::DefaultSample;',
    ''
) + $sample + @('')
Set-Content -Path 'crates/reafl-core/src/audio/sample.rs' -Value $sampleFile -Encoding utf8

$outputFile = @(
    '//! The UI-side half of the audio engine: device setup and the command handle.',
    '',
    'use super::*;',
    ''
) + $output + @('')
Set-Content -Path 'crates/reafl-core/src/audio/output.rs' -Value $outputFile -Encoding utf8

$rest = $lines[0..($A - 1)] + $lines[$B..($C - 1)] + $lines[$D..($lines.Count - 1)]
Set-Content -Path $src -Value $rest -Encoding utf8

foreach ($f in 'model.rs','sample.rs','output.rs') { "{0,5} {1}" -f (Get-Content "crates/reafl-core/src/audio/$f").Count, $f }
Get-Content 'crates/reafl-core/src/audio/mod.rs' | Select-Object -First 40
