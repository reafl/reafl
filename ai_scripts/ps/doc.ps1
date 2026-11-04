<#
.SYNOPSIS
    Builds the API documentation for every crate.

.DESCRIPTION
    Runs `cargo doc --workspace --no-deps --document-private-items` and opens
    the result. All modules in the workspace are documented with rustdoc
    comments, so this doubles as the developer manual for the engine, the
    profiler and their macros.

.EXAMPLE
    pwsh -File ai_scripts/ps/doc.ps1
#>
[CmdletBinding()]
param(
    # Skip opening the browser afterwards.
    [switch]$NoOpen
)

$ErrorActionPreference = 'Stop'
& cargo doc --workspace --no-deps --document-private-items
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if (-not $NoOpen) {
    Start-Process 'target/doc/pulseforge_profiling/index.html'
}
exit 0
