param([string]$Root = "crates/pulseforge-core/src/audio")

$common = Get-Content -LiteralPath (Join-Path $Root 'model_imports.txt')

$modules = @{
    'engine.rs' = @('use super::model::*;', 'use super::dsp::*;')
    'dsp.rs' = @('use super::model::*;', 'use super::engine::*;')
    'tests.rs' = @('use super::model::*;', 'use super::engine::*;', 'use super::dsp::*;')
}

foreach ($entry in $modules.GetEnumerator()) {
    $path = Join-Path $Root $entry.Key
    $lines = Get-Content -LiteralPath $path
    $doc = @()
    foreach ($line in $lines) { if ($line -match '^//!') { $doc += $line } }
    $bodyStart = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^(impl |pub fn |fn |pub struct |struct |pub enum |enum |const |static |#\[derive|#\[cfg)') {
            $bodyStart = $i
            break
        }
    }
    $body = $lines[$bodyStart..($lines.Count - 1)]
    Set-Content -LiteralPath $path -Value ($doc + @('') + $common + @('') + $entry.Value + @('') + $body)
    Write-Host "rebuilt $($entry.Key)"
}
