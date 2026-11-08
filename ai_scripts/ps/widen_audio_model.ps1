param([string]$Path = "crates/reafl-core/src/audio/model.rs")

# Everything in the data module is crate-internal to the engine; the sibling
# modules reach into these fields and helpers directly.
$lines = Get-Content -LiteralPath $Path
$out = foreach ($line in $lines) {
    if ($line -match '^    fn ') { $line -replace '^    fn ', '    pub(crate) fn ' }
    elseif ($line -match '^    (\w+):') { $line -replace '^    ', '    pub(crate) ' }
    else { $line }
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'model fields and methods widened'
