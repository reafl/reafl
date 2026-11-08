param([string]$Root = "crates/reafl-core/src/project")

$source = Join-Path $Root 'model.rs'
$folder = Join-Path $Root 'model'
if (-not (Test-Path $folder)) { New-Item -ItemType Directory -Force $folder | Out-Null }

$lines = Get-Content -LiteralPath $source
# The imports at the top of the monolith are repeated on every slice.
$header = $lines[0..5]

function Write-Slice {
    param([string]$Name, [string]$Title, [int]$From, [int]$To)
    $path = Join-Path $folder $Name
    Set-Content -LiteralPath $path -Value (@($Title) + @('') + $header + @('') + $lines[$From..$To])
    Write-Host "$Name <- $From..$To"
}

Write-Slice 'params.rs' '//! Time, notes, arpeggios and the constants the model starts from.' 6 124
Write-Slice 'targets.rs' '//! Automation sources and the parameter enums that automation addresses.' 125 742
Write-Slice 'automation.rs' '//! Automation points and tracks, and the insert slots they can drive.' 743 880
Write-Slice 'arrangement.rs' '//! Instruments, patterns, clips, lanes, the master channel and the project itself.' 881 1119

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
