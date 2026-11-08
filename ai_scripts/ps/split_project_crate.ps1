param([string]$Root = "crates/reafl-core/src")

$source = Join-Path $Root 'project.rs'
$target = Join-Path $Root 'project'
if (-not (Test-Path $target)) { New-Item -ItemType Directory -Force $target | Out-Null }

$lines = Get-Content -LiteralPath $source

# The file is already in three natural layers: the data types, the `impl Project`
# behaviour, and the tests.
$modelEnd = 1203
$implEnd = 1895

Set-Content -LiteralPath (Join-Path $target 'model.rs') -Value (@(
    '//! The project data model: values, targets and the structures they fill.',
    '',
    '#![allow(unused_imports)]',
    ''
) + $lines[0..($modelEnd - 1)])

Set-Content -LiteralPath (Join-Path $target 'behaviour.rs') -Value (@(
    '//! What a [`Project`] can do: normalising, automation lookup, lane edits.',
    '',
    'use super::*;',
    ''
) + $lines[$modelEnd..($implEnd - 1)])

Set-Content -LiteralPath (Join-Path $target 'tests.rs') -Value (@(
    '//! Tests for the project model and its behaviour.',
    '',
    'use super::*;',
    ''
) + $lines[$implEnd..($lines.Count - 1)])

Set-Content -LiteralPath (Join-Path $target 'mod.rs') -Value @(
    '//! The project model and the operations on it.',
    '//!',
    '//! [`Project`] is the whole saved document: patterns, arrangement lanes,',
    '//! instruments, mixer routing and automation. It is split into the data',
    '//! definitions in [`model`] and the behaviour in [`behaviour`], and both are',
    '//! re-exported here so callers keep writing `project::Project`.',
    '',
    'mod behaviour;',
    'mod model;',
    '',
    '#[cfg(test)]',
    'mod tests;',
    '',
    'pub use behaviour::*;',
    'pub use model::*;'
)

Remove-Item -LiteralPath $source
Write-Host 'project.rs split into project/{mod,model,behaviour,tests}.rs'
