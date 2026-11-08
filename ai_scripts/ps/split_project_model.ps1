param([string]$Root = "crates/reafl-core/src/project")

$source = Join-Path $Root 'model.rs'
$folder = Join-Path $Root 'model'
if (-not (Test-Path $folder)) { New-Item -ItemType Directory -Force $folder | Out-Null }

$lines = Get-Content -LiteralPath $source
$header = $lines[0..5]        # module doc and imports

function Slice([int]$From, [int]$To) { $lines[$From..$To] }

# The file is already grouped by topic; the cuts land between those groups.
$params = Slice 6 287        # time, notes, arpeggiator and voice settings
$enums = Slice 288 743       # parameter enums and targets
$automation = Slice 744 881   # automation points, tracks and inserts
$arrangement = Slice 882 1072 # instruments, patterns, clips, lanes and master
$project = Slice 1073 1119    # the Project document itself

Set-Content -LiteralPath (Join-Path $folder 'params.rs') -Value (@(
    '//! Time, notes, arpeggios and the per-instrument voice settings.'
) + $params)

Set-Content -LiteralPath (Join-Path $folder 'targets.rs') -Value (@(
    '//! The parameter enums and the [`ParameterTarget`] automation addresses.'
) + $enums)

Set-Content -LiteralPath (Join-Path $folder 'automation.rs') -Value (@(
    '//! Automation points and tracks, and the insert slots they can drive.'
) + $automation)

Set-Content -LiteralPath (Join-Path $folder 'arrangement.rs') -Value (@(
    '//! Instruments, patterns, clips, lanes and the master channel.'
) + $arrangement)

Set-Content -LiteralPath (Join-Path $folder 'document.rs') -Value (@(
    '//! The [`Project`] document itself: what a saved session contains.'
) + $project)

Set-Content -LiteralPath (Join-Path $folder 'mod.rs') -Value @(
    '//! The project data model, split by topic.',
    '//!',
    '//! Everything here is plain data with small helpers: [`params`] holds time and',
    '//! notes, [`targets`] the automatable parameter addresses, [`automation`] the',
    '//! lanes themselves, [`arrangement`] the instruments and clips, and',
    '//! [`document`] ties them together in [`Project`]. The behaviour lives in',
    '//! [`crate::project::behaviour`].',
    '',
    'mod arrangement;',
    'mod automation;',
    'mod document;',
    'mod params;',
    'mod targets;',
    '',
    'pub use arrangement::*;',
    'pub use automation::*;',
    'pub use document::*;',
    'pub use params::*;',
    'pub use targets::*;'
)

# The imports the monolith had at the top belong on every slice.
$imports = $header | Where-Object { $_ -match '^use |^    |^\};' }
foreach ($name in @('params.rs', 'targets.rs', 'automation.rs', 'arrangement.rs', 'document.rs')) {
    $path = Join-Path $folder $name
    $body = Get-Content -LiteralPath $path
    $doc = @()
    foreach ($line in $body) { if ($line -match '^//!') { $doc += $line } }
    $rest = $body | Where-Object { $_ -notmatch '^//!' -and $_ -notmatch '^use ' -and $_ -ne '' }
    Set-Content -LiteralPath $path -Value ($doc + @('') + $imports + @('') + $rest)
}

Remove-Item -LiteralPath $source
Write-Host 'project/model.rs split into project/model/'
