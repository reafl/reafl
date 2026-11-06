param([string]$Root = "crates/pulseforge-app/src")

# Rebuilds state.rs from the committed monolith, widening only struct fields and
# leaving enum variants alone.
$original = & git show HEAD:crates/pulseforge-app/src/lib.rs
$chunk = $original[169..332]

$out = @()
$kind = ''
$depth = 0
foreach ($line in $chunk) {
    if ($line -match '^(struct|enum) (\w+)') {
        $kind = $Matches[1]
        $depth = 1
        $out += ($line -replace '^(struct|enum) ', 'pub(crate) $1 ')
        continue
    }
    if ($depth -gt 0 -and $line -match '^\}') { $depth = 0; $kind = ''; $out += $line; continue }
    if ($depth -eq 1 -and $kind -eq 'struct' -and $line -match '^    (\w+):') {
        $out += ($line -replace '^    ', '    pub(crate) ')
        continue
    }
    $out += $line
}

# Drop a trailing doc comment that belongs to the next item in the monolith.
while ($out.Count -gt 0 -and ($out[$out.Count - 1].Trim() -eq '' -or $out[$out.Count - 1] -match '^\s*(///|#\[)')) {
    $out = $out[0..($out.Count - 2)]
}

Set-Content -LiteralPath (Join-Path $Root 'state.rs') -Value (@(
    '//! Small state types: windows, selections, gestures and rows.',
    '',
    'use crate::*;',
    ''
) + $out)
Write-Host 'state.rs rebuilt'
