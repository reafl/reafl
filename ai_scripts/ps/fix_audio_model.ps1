param([string]$Path = "crates/pulseforge-core/src/audio/model.rs")

# The sibling modules need the engine type, a method and two field groups that the
# monolith had all in one file.
$lines = Get-Content -LiteralPath $Path
$out = @()
$inEqState = $false
foreach ($line in $lines) {
    if ($line -match '^pub\(crate\) struct EqState \{') { $inEqState = $true; $out += $line; continue }
    if ($inEqState -and $line -match '^\}') { $inEqState = $false; $out += $line; continue }
    if ($inEqState -and $line -match '^    (\w+):') { $out += ($line -replace '^    ', '    pub(crate) '); continue }
    if ($line -match '^    fn identity\(') { $out += ($line -replace '^    fn ', '    pub(crate) fn '); continue }
    if ($line -match 'AudioEngine::new\(') {
        $out += ($line -replace 'AudioEngine::new\(', 'super::engine::AudioEngine::new(')
        continue
    }
    $out += $line
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'model visibility patched'
