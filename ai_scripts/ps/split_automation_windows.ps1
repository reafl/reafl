$path = 'crates/reafl-app/src/views/automation.rs'
$lines = Get-Content $path
$start = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match 'fn render_export_window') { $start = $i; break }
}
if ($start -lt 0) { throw 'start not found' }
$end = -1
for ($i = $start + 1; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -eq '}') { $end = $i; break }
}
if ($end -lt 0) { throw 'end not found' }

$before = $lines[0..($start - 1)]
while ($before.Count -gt 0 -and $before[$before.Count - 1].Trim() -eq '') {
    $before = $before[0..($before.Count - 2)]
}
$before += '}'

$methods = $lines[$start..($end - 1)]
while ($methods.Count -gt 0 -and $methods[$methods.Count - 1].Trim() -eq '') {
    $methods = $methods[0..($methods.Count - 2)]
}

$after = $lines[($end + 1)..($lines.Count - 1)]

$header = @(
    '//! The offline export window and the audio sample-clip window.',
    '',
    'use crate::*;',
    '',
    'impl ReaflApp {'
)
Set-Content -Path 'crates/reafl-app/src/views/clip_windows.rs' -Value ($header + $methods + @('}', '')) -Encoding utf8

Set-Content -Path $path -Value ($before + $after) -Encoding utf8

"cut lines $($start+1)..$($end+1)"
"automation.rs -> $((Get-Content $path).Count) lines"
"clip_windows.rs -> $((Get-Content 'crates/reafl-app/src/views/clip_windows.rs').Count) lines"
