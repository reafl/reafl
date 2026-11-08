$dir = 'crates/reafl-core/src/project/behaviour'
foreach ($f in Get-ChildItem $dir -Filter *.rs) {
    $lines = Get-Content $f.FullName
    if ($lines[0] -notmatch 'use super::\*;\s+impl Project \{') { continue }
    $doc = ($lines[0] -split '  use super')[0].TrimEnd()
    $rest = $lines[1..($lines.Count - 1)]
    $new = @($doc, '', 'use super::*;', '', 'impl Project {') + $rest
    Set-Content -Path $f.FullName -Value $new -Encoding utf8
    "fixed: $($f.Name)"
}
