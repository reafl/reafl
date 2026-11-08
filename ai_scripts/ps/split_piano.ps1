$src = 'crates/reafl-app/src/views/piano.rs'
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

$C = Doc-Start (Find-Index '^    pub\(crate\) fn piano_grid' 0)
"anchors C=$C count=$($lines.Count)"

$head = $lines[0..($C - 1)]
while ($head.Count -gt 0 -and $head[$head.Count - 1].Trim() -eq '') { $head = $head[0..($head.Count - 2)] }
$grid = $lines[$C..($lines.Count - 1)]
while ($grid.Count -gt 0 -and $grid[$grid.Count - 1].Trim() -eq '') { $grid = $grid[0..($grid.Count - 2)] }
if ($grid[$grid.Count - 1].Trim() -eq '}') { $grid = $grid[0..($grid.Count - 2)] }

Set-Content -Path 'crates/reafl-app/src/views/piano_grid.rs' -Value (@(
    '//! The piano-roll note grid and its box-selection helpers.',
    '',
    'use crate::*;',
    '',
    'impl ReaflApp {'
) + $grid + @('}', '')) -Encoding utf8

Set-Content -Path $src -Value ($head + @('}', '')) -Encoding utf8

Get-ChildItem 'crates/reafl-app/src/views/piano*.rs' | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
