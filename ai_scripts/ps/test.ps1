<#
.SYNOPSIS
    Runs the whole workspace test suite.

.DESCRIPTION
    Runs `cargo test --workspace`. The hosting crate keeps its real-bundle
    tests `#[ignore]`d because they need installed VST3/CLAP plug-ins; run them
    with `cargo test -p pulseforge-core -- --ignored` on a machine that has
    some.

.EXAMPLE
    pwsh -File ai_scripts/ps/test.ps1
#>
[CmdletBinding()]
param(
    # Also run the ignored tests that need installed plug-in bundles.
    [switch]$IncludeIgnored
)

$ErrorActionPreference = 'Stop'
if ($IncludeIgnored) {
    & cargo test --workspace -- --include-ignored
} else {
    & cargo test --workspace
}
exit $LASTEXITCODE
