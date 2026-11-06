$src = 'crates/pulseforge-app/src/views/settings.rs'
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

$C = Doc-Start (Find-Index '^    pub\(crate\) fn render_sample_browser' 0)
"anchors C=$C count=$($lines.Count)"

$head = $lines[0..($C - 1)]
while ($head.Count -gt 0 -and $head[$head.Count - 1].Trim() -eq '') { $head = $head[0..($head.Count - 2)] }
$tail = $lines[$C..($lines.Count - 1)]
while ($tail.Count -gt 0 -and $tail[$tail.Count - 1].Trim() -eq '') { $tail = $tail[0..($tail.Count - 2)] }
if ($tail[$tail.Count - 1].Trim() -eq '}') { $tail = $tail[0..($tail.Count - 2)] }

Set-Content -Path 'crates/pulseforge-app/src/views/sample_browser.rs' -Value (@(
    '//! The sample browser and the loading helpers it calls.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {'
) + $tail + @('}', '')) -Encoding utf8

Set-Content -Path $src -Value ($head + @('}', '')) -Encoding utf8

foreach ($f in 'crates/pulseforge-app/src/views/settings.rs','crates/pulseforge-app/src/views/sample_browser.rs') { "{0,5} {1}" -f (Get-Content $f).Count, $f }
