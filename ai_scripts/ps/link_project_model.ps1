param([string]$Root = "crates/reafl-core/src/project/model")

# The slices reference each other's types, so each module imports its siblings.
$siblings = @{
    'params.rs' = @()
    'targets.rs' = @('use super::params::*;')
    'automation.rs' = @('use super::params::*;', 'use super::targets::*;')
    'arrangement.rs' = @('use super::automation::*;', 'use super::params::*;', 'use super::targets::*;')
}

foreach ($entry in $siblings.GetEnumerator()) {
    $path = Join-Path $Root $entry.Key
    $lines = Get-Content -LiteralPath $path
    $insertAt = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^pub |^impl |^#\[') { $insertAt = $i; break }
    }
    $head = $lines[0..($insertAt - 1)]
    $rest = $lines[$insertAt..($lines.Count - 1)]
    Set-Content -LiteralPath $path -Value ($head + $entry.Value + $rest)
    Write-Host "linked $($entry.Key)"
}
