param([string]$Path = "crates/pulseforge-widgets/src/control.rs")

# Adds the `compact` field to every `Spec` initialiser.
$lines = Get-Content -LiteralPath $Path
$out = @()
foreach ($line in $lines) {
    $out += $line
    if ($line -match '^\s+width: None,$') {
        $indent = ($line -replace 'width: None,.*$', '')
        $out += ($indent + 'compact: false,')
    }
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'compact flag added to the spec initialisers'
