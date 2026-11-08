$src = 'crates/reafl-core/src/audio/engine/mod.rs'
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

$A = Doc-Start (Find-Index '^    pub\(crate\) fn apply_smoothing_settings' 0)
$B = Doc-Start (Find-Index '^    /// Groups enabled automation lanes by target' 0)
$C = Doc-Start (Find-Index '^    pub\(crate\) fn resize_effect_parameters' 0)
"anchors A=$A B=$B C=$C count=$($lines.Count)"

$setup1 = $lines[$A..($B - 1)]
$indexBlock = $lines[$B..($C - 1)]
$setup2 = $lines[$C..($lines.Count - 2)]

$setupFile = @(
    '//! Deriving the engine''s cached state after a project change.',
    '//!',
    '//! Everything here rebuilds flattened caches (smoothed values, routing,',
    '//! effect parameter slots, send edges) from the project, so the sample loop',
    '//! reads arrays instead of walking the document.',
    '',
    'use super::*;',
    '',
    'impl AudioEngine {'
) + $setup1 + @('') + $setup2 + @('}', '')
Set-Content -Path 'crates/reafl-core/src/audio/engine/setup.rs' -Value $setupFile -Encoding utf8

$head = $lines[0..($A - 1)]
while ($head.Count -gt 0 -and $head[$head.Count - 1].Trim() -eq '') { $head = $head[0..($head.Count - 2)] }
Set-Content -Path $src -Value ($head + @('') + $indexBlock + @('}', '')) -Encoding utf8

foreach ($f in 'mod.rs','setup.rs') { "{0,5} {1}" -f (Get-Content "crates/reafl-core/src/audio/engine/$f").Count, $f }
