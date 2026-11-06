param([string]$Source = "crates/pulseforge-app/src/lib.rs", [string]$Root = "crates/pulseforge-app/src")

$lines = Get-Content -LiteralPath $Source

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
    Write-Host "$Name <- $From..$To"
}

# Cut from the bottom up so the line numbers above stay valid.
Cut-Range 1516 1525 'hosted_meta.rs' 'The cached static data read from each hosted plug-in.'
Cut-Range 1053 1391 'update.rs' 'The per-frame update loop: input, audio commands and the window wiring.'
Cut-Range 709 878 'app_state.rs' 'The editor state itself.'
Cut-Range 334 708 'layout.rs' 'Arrangement layout maths and the previews drawn inside clips.'
Cut-Range 170 333 'state.rs' 'Small state types: windows, selections, gestures and rows.'
Cut-Range 109 169 'cursor.rs' 'The custom SVG tool cursors.'

$remaining = $lines[0..108] + $lines[1526..($lines.Count - 1)]
Set-Content -LiteralPath $Source -Value $remaining
Write-Host 'lib.rs trimmed'
