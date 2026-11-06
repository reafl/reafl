param([string]$Path = "crates/pulseforge-core/src/audio/engine.rs")

# The offline renderer in `dsp` drives the engine directly, so the engine's
# fields and helpers have to be visible inside the crate.
$lines = Get-Content -LiteralPath $Path
$out = @()
$inEngineStruct = $false
foreach ($line in $lines) {
    if ($line -match '^pub\(crate\) struct AudioEngine \{') { $inEngineStruct = $true; $out += $line; continue }
    if ($inEngineStruct -and $line -match '^\}') { $inEngineStruct = $false; $out += $line; continue }
    if ($inEngineStruct -and $line -match '^    (\w+):') { $out += ($line -replace '^    ', '    pub(crate) '); continue }
    if ($line -match '^    fn ') { $out += ($line -replace '^    fn ', '    pub(crate) fn '); continue }
    $out += $line
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'engine visibility patched'
