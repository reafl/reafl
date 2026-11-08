param([string]$Dir = "crates/reafl-app/src")

# Moves trailing doc comments that were cut with the wrong module back to the
# item they belong to.
function Move-Tail($from, $to, $count) {
    $fromPath = Join-Path $Dir $from
    $toPath = Join-Path $Dir $to
    $lines = Get-Content -LiteralPath $fromPath
    $tail = $lines[($lines.Count - $count)..($lines.Count - 1)]
    Set-Content -LiteralPath $fromPath -Value $lines[0..($lines.Count - $count - 1)]
    $target = Get-Content -LiteralPath $toPath
    $head = $target[0..3]
    $rest = $target[4..($target.Count - 1)]
    Set-Content -LiteralPath $toPath -Value ($head + $tail + $rest)
    Write-Host "moved $count line(s) from $from to $to"
}

Move-Tail geometry.rs icons.rs 6
Move-Tail icons.rs presets.rs 1
Move-Tail presets.rs tooltip.rs 2
