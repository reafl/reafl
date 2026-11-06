$p = 'crates/pulseforge-settings/src/settings/tests.rs'
$lines = Get-Content $p
$start = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^mod tests \{') { $start = $i; break }
}
if ($start -lt 0) { throw 'mod tests not found' }
$body = $lines[($start + 1)..($lines.Count - 1)]
# drop trailing blank lines and the closing brace of the old module
while ($body.Count -gt 0 -and $body[$body.Count - 1].Trim() -eq '') { $body = $body[0..($body.Count - 2)] }
if ($body[$body.Count - 1].Trim() -eq '}') { $body = $body[0..($body.Count - 2)] }
$body = $body | ForEach-Object { if ($_ -match '^    ') { $_.Substring(4) } else { $_ } }
$out = @('//! Unit tests for the settings model.', '', 'use super::*;', '') + $body + @('')
Set-Content $p -Value $out -Encoding utf8
Get-Content $p -First 6
"---"
Get-Content $p -Tail 3
