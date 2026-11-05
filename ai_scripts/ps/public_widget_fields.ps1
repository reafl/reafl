param([string]$Dir = "crates/pulseforge-widgets/src")

# Makes the public widget data structures' fields visible to the other windows.
$targets = @('power.rs', 'mixer_strip.rs', 'knob.rs')
foreach ($name in $targets) {
    $path = Join-Path $Dir $name
    $lines = Get-Content -LiteralPath $path
    $inside = $false
    $out = foreach ($line in $lines) {
        if ($line -match '^pub struct ') { $inside = $true; $line; continue }
        if ($inside -and $line -match '^\}') { $inside = $false; $line; continue }
        if ($inside -and $line -match '^    (\w+):') { $line -replace '^    ', '    pub ' }
        elseif ($inside -and $line -match '^    pub ') { $line }
        else { $line }
    }
    Set-Content -LiteralPath $path -Value $out
    Write-Host "public fields of $name"
}
