param([string]$Source = "crates/pulseforge-app/src/lib.rs", [string]$Root = "crates/pulseforge-app/src")

$lines = Get-Content -LiteralPath $Source
$taken = New-Object 'System.Collections.Generic.HashSet[int]'

function Widen([string[]]$Content) {
    $Content | ForEach-Object {
        if ($_ -match '^(struct|enum|const|fn|static) ') { $_ -replace '^(struct|enum|const|fn|static) ', 'pub(crate) $1 ' }
        else { $_ }
    }
}

function Cut-Range {
    param([int]$From, [int]$To, [string]$Name, [string]$Title)
    $path = Join-Path $Root $Name
    $chunk = Widen $lines[($From - 1)..($To - 1)]
    Set-Content -LiteralPath $path -Value (@("//! $Title", '', 'use crate::*;', '') + $chunk)
    for ($i = $From; $i -le $To; $i++) { [void]$taken.Add($i) }
    Write-Host "$Name <- $From..$To"
}

Cut-Range 109 169 'cursor.rs' 'The custom SVG tool cursors.'
Cut-Range 170 333 'state.rs' 'Small state types: windows, selections, gestures and rows.'
Cut-Range 334 708 'layout.rs' 'Arrangement layout maths and the previews drawn inside clips.'
Cut-Range 709 878 'app_state.rs' 'The editor state itself.'
Cut-Range 1053 1391 'update.rs' 'The per-frame update loop: input, audio commands and the window wiring.'
Cut-Range 1516 1525 'hosted_meta.rs' 'The cached static data read from each hosted plug-in.'

$remaining = @()
for ($i = 1; $i -le $lines.Count; $i++) {
    if (-not $taken.Contains($i)) { $remaining += $lines[$i - 1] }
}
Set-Content -LiteralPath $Source -Value $remaining
Write-Host "lib.rs keeps $($remaining.Count) lines"
