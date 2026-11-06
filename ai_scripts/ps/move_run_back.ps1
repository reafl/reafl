param([string]$Root = "crates/pulseforge-app/src")

# Moves the entry point out of the cursor module and back to the crate root.
$cursorPath = Join-Path $Root 'cursor.rs'
$libPath = Join-Path $Root 'lib.rs'

$cursor = Get-Content -LiteralPath $cursorPath
$start = 0
for ($i = 0; $i -lt $cursor.Count; $i++) {
    if ($cursor[$i] -match '^/// Starts the PulseForge editor') { $start = $i; break }
}
$runBlock = $cursor[$start..($cursor.Count - 1)]
$kept = $cursor[0..($start - 1)]
# Trim the blank lines the block left behind.
while ($kept.Count -gt 0 -and $kept[$kept.Count - 1].Trim() -eq '') { $kept = $kept[0..($kept.Count - 2)] }
Set-Content -LiteralPath $cursorPath -Value $kept

# The entry point goes after the shared imports at the top of lib.rs.
$lib = Get-Content -LiteralPath $libPath
$insertAt = 0
for ($i = 0; $i -lt $lib.Count; $i++) {
    if ($lib[$i] -match '^const BAR_WIDTH') { $insertAt = $i; break }
}
$head = $lib[0..($insertAt - 1)]
$rest = $lib[$insertAt..($lib.Count - 1)]
Set-Content -LiteralPath $libPath -Value ($head + $runBlock + @('') + $rest)
Write-Host 'run() moved back to lib.rs'
