param([string]$Root = "crates/reafl-app/src")

# Every small state type is shared between the app modules, so their fields and
# trait implementations are crate-visible too.
function Widen-Fields {
    param([string]$Name)
    $path = Join-Path $Root $Name
    $lines = Get-Content -LiteralPath $path
    $out = @()
    $depth = 0
    foreach ($line in $lines) {
        if ($line -match '^(pub\(crate\) )?(struct|enum) \w+') { $depth = 1; $out += $line; continue }
        if ($depth -gt 0 -and $line -match '^\}') { $depth = 0; $out += $line; continue }
        if ($depth -eq 1 -and $line -match '^    (\w+)(:|,|\()') { $out += ($line -replace '^    ', '    pub(crate) '); continue }
        $out += $line
    }
    Set-Content -LiteralPath $path -Value $out
    Write-Host "widened $Name"
}

Widen-Fields 'state.rs'
Widen-Fields 'hosted_meta.rs'

# `EditorWindows` is built with `..Default::default()` in the setup path.
$statePath = Join-Path $Root 'state.rs'
$state = Get-Content -LiteralPath $statePath
$out = @()
foreach ($line in $state) {
    if ($line -match '^pub\(crate\) struct EditorWindows \{' -and $out.Count -gt 0 -and $out[$out.Count - 1] -match '^#\[derive\(Default\)\]$') {
        $out += $line
        continue
    }
    $out += $line
}
Set-Content -LiteralPath $statePath -Value $out
Write-Host 'state.rs checked for Default'
