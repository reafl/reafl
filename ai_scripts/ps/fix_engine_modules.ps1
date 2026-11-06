param([string]$Root = "crates/pulseforge-core/src/audio/engine")

# The split left a duplicate module doc in the root and an extra brace at the end
# of the last chunk.
$modPath = Join-Path $Root 'mod.rs'
$modLines = Get-Content -LiteralPath $modPath
$out = @()
$seenDoc = $false
foreach ($line in $modLines) {
    if ($line -match '^//! The render engine: mixing') { continue }
    if ($line -match '^//! The render engine: state') { $seenDoc = $true }
    if ($line -match '^mod (automation|effects|render);$' -and -not $seenDoc) { $out += $line; continue }
    $out += $line
}
Set-Content -LiteralPath $modPath -Value $out

$renderPath = Join-Path $Root 'render.rs'
$renderLines = Get-Content -LiteralPath $renderPath
while ($renderLines.Count -gt 0 -and $renderLines[$renderLines.Count - 1].Trim() -eq '') {
    $renderLines = $renderLines[0..($renderLines.Count - 2)]
}
if ($renderLines[$renderLines.Count - 1].Trim() -eq '}' -and $renderLines[$renderLines.Count - 2].Trim() -eq '}') {
    $renderLines = $renderLines[0..($renderLines.Count - 2)]
}
Set-Content -LiteralPath $renderPath -Value $renderLines
Write-Host 'engine modules repaired'
