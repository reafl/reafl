$src = 'crates/pulseforge-app/src/lib.rs'
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

$A = Find-Index '^impl PulseForgeApp \{$' 0
$B = Find-Index '^impl PulseForgeApp \{\}$' 0
$C = Find-Index '^impl PulseForgeApp \{$' ($B + 1)
$D = Doc-Start (Find-Index '^    fn render_debug_panel' 0)
$E = Doc-Start (Find-Index '^    fn render_plugin_windows' 0)
$T = Find-Index '^#\[cfg\(test\)\]' 0
"anchors A=$A B=$B C=$C D=$D E=$E T=$T count=$($lines.Count)"

$editing = $lines[($A + 1)..($B - 2)] | ForEach-Object { $_ -replace '^    fn ', '    pub(crate) fn ' }
$debug = $lines[$D..($E - 1)]
$pluginWindows = $lines[$E..($T - 1)]
while ($pluginWindows.Count -gt 0 -and $pluginWindows[$pluginWindows.Count - 1].Trim() -eq '') { $pluginWindows = $pluginWindows[0..($pluginWindows.Count - 2)] }
# trim the closing braces of the nested impl blocks
while ($pluginWindows.Count -gt 0 -and $pluginWindows[$pluginWindows.Count - 1].Trim() -eq '}') { $pluginWindows = $pluginWindows[0..($pluginWindows.Count - 2)] }

$testBody = $lines[($T + 3)..($lines.Count - 1)]

$editingFile = @(
    '//! Editing helpers that only touch project state.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {'
) + $editing + @('}', '')
Set-Content -Path 'crates/pulseforge-app/src/editing.rs' -Value $editingFile -Encoding utf8

$debugFile = @(
    '//! The debug and profiler window.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {'
) + $debug + @('}', '')
Set-Content -Path 'crates/pulseforge-app/src/debug_panel.rs' -Value $debugFile -Encoding utf8

$windowsFile = @(
    '//! The floating windows of hosted and built-in plug-ins.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {'
) + $pluginWindows + @('}', '')
Set-Content -Path 'crates/pulseforge-app/src/views/plugin_windows.rs' -Value $windowsFile -Encoding utf8

$testsFile = @(
    '//! Unit tests for the editor state and its helpers.',
    '',
    'use crate::*;'
) + $testBody[1..($testBody.Count - 1)] + @('')
# drop the closing brace of the old `mod tests`
while ($testsFile.Count -gt 0 -and $testsFile[$testsFile.Count - 1].Trim() -eq '') { $testsFile = $testsFile[0..($testsFile.Count - 2)] }
if ($testsFile[$testsFile.Count - 1].Trim() -eq '}') { $testsFile = $testsFile[0..($testsFile.Count - 2)] }
Set-Content -Path 'crates/pulseforge-app/src/tests.rs' -Value ($testsFile + @('')) -Encoding utf8

$keep = $lines[0..($A - 1)]
Set-Content -Path $src -Value ($keep + @('')) -Encoding utf8

Get-ChildItem 'crates/pulseforge-app/src/*.rs' | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
