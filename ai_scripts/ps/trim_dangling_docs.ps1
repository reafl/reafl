param([string]$Root = "crates/pulseforge-app/src")

# Trims doc or attribute lines that a cut left dangling at the end of a module.
foreach ($name in @('cursor.rs', 'hosted_meta.rs', 'state.rs', 'layout.rs', 'app_state.rs', 'update.rs')) {
    $path = Join-Path $Root $name
    $lines = Get-Content -LiteralPath $path
    $count = 0
    for ($i = $lines.Count - 1; $i -ge 0; $i--) {
        if ($lines[$i].Trim() -eq '' -or $lines[$i] -match '^\s*(///|#\[)') { $count++ } else { break }
    }
    if ($count -gt 0) {
        Set-Content -LiteralPath $path -Value $lines[0..($lines.Count - $count - 1)]
        Write-Host "trimmed $count line(s) from $name"
    }
}
