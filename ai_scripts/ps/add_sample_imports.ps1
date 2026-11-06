$files = @(
    'crates/pulseforge-core/src/audio/dsp.rs',
    'crates/pulseforge-core/src/audio/model.rs',
    'crates/pulseforge-core/src/audio/engine/mod.rs',
    'crates/pulseforge-core/src/audio/engine/commands.rs',
    'crates/pulseforge-core/src/audio/engine/render.rs',
    'crates/pulseforge-core/src/audio/engine/voices.rs',
    'crates/pulseforge-core/src/audio/engine/automation.rs',
    'crates/pulseforge-core/src/audio/engine/setup.rs'
)
foreach ($p in $files) {
    if (-not (Test-Path $p)) { continue }
    $c = Get-Content $p
    if (($c -join "`n") -match 'use super::sample::\*') { continue }
    $out = New-Object System.Collections.ArrayList
    $inserted = $false
    foreach ($line in $c) {
        [void]$out.Add($line)
        if (-not $inserted -and $line -match '^use super::model::\*;$') {
            [void]$out.Add('use super::sample::*;')
            $inserted = $true
        }
    }
    if ($inserted) {
        Set-Content -Path $p -Value $out -Encoding utf8
        "patched $p"
    } else {
        "skipped $p"
    }
}
