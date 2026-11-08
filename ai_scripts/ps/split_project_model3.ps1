param([string]$Root = "crates/reafl-core/src/project")

$source = Join-Path $Root 'model.rs'
$folder = Join-Path $Root 'model'
if (-not (Test-Path $folder)) { New-Item -ItemType Directory -Force $folder | Out-Null }

$lines = Get-Content -LiteralPath $source
$header = $lines[0..5]

# Item starts, so the cuts land between whole items instead of inside them.
$itemStart = @(-1)
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^(#\[derive|#\[serde|#\[cfg|pub (struct|enum|fn|const)|impl |pub fn)') { $itemStart += $i }
}

function First-After([int]$Target) {
    foreach ($index in $itemStart) { if ($index -ge $Target) { return $index } }
    return $lines.Count
}

$b1 = First-After 118
$b2 = First-After 738
$b3 = First-After 876

function Write-Slice {
    param([string]$Name, [string]$Title, [int]$From, [int]$To)
    $path = Join-Path $folder $Name
    Set-Content -LiteralPath $path -Value (@($Title) + @('') + $header + @('') + $lines[$From..$To])
    Write-Host "$Name <- $From..$To"
}

Write-Slice 'params.rs' '//! Time, notes, arpeggios and the constants the model starts from.' 6 ($b1 - 1)
Write-Slice 'targets.rs' '//! Automation sources and the parameter enums that automation addresses.' $b1 ($b2 - 1)
Write-Slice 'automation.rs' '//! Automation points and tracks, and the insert slots they can drive.' $b2 ($b3 - 1)
Write-Slice 'arrangement.rs' '//! Instruments, patterns, clips, lanes, the master channel and the project itself.' $b3 ($lines.Count - 1)

Set-Content -LiteralPath (Join-Path $folder 'mod.rs') -Value @(
    '//! The project data model, split by topic.',
    '//!',
    '//! Everything here is plain data with small helpers: [`params`] holds time and',
    '//! notes, [`targets`] the automatable parameter addresses, [`automation`] the',
    '//! lanes themselves, and [`arrangement`] the instruments, clips and the',
    '//! [`Project`] document that ties them together. The behaviour lives in',
    '//! [`crate::project::behaviour`].',
    '',
    'mod arrangement;',
    'mod automation;',
    'mod params;',
    'mod targets;',
    '',
    'pub use arrangement::*;',
    'pub use automation::*;',
    'pub use params::*;',
    'pub use targets::*;'
)

Remove-Item -LiteralPath $source
Write-Host 'project/model.rs split'
