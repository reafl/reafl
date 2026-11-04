<#
.SYNOPSIS
    Type-checks every crate, test and example in the workspace.

.DESCRIPTION
    Runs `cargo check --workspace --all-targets`. This is the fastest way to
    verify a change compiles without linking; warnings are printed too, so a
    run with no output means the tree is clean.

.EXAMPLE
    pwsh -File ai_scripts/ps/check.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
& cargo check --workspace --all-targets --message-format short
exit $LASTEXITCODE
