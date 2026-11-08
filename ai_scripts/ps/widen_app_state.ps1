param([string]$Path = "crates/reafl-app/src/app_state.rs")

# The editor's methods live in sibling modules, so its fields are crate-visible.
$lines = Get-Content -LiteralPath $Path
$out = @()
$inStruct = $false
foreach ($line in $lines) {
    if ($line -match '^(pub\(crate\) )?struct ReaflApp \{') { $inStruct = $true; $out += $line; continue }
    if ($inStruct -and $line -match '^\}') { $inStruct = $false; $out += $line; continue }
    if ($inStruct -and $line -match '^    (\w+):') { $out += ($line -replace '^    ', '    pub(crate) '); continue }
    $out += $line
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'ReaflApp fields widened'
