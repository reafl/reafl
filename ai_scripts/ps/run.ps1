<#
.SYNOPSIS
    Runs PulseForge, forwarding any extra arguments to the app.

.DESCRIPTION
    Thin wrapper around `cargo run --` so command lines do not need repeating.
    Useful examples:

        pwsh -File ai_scripts/ps/run.ps1 --debug
        pwsh -File ai_scripts/ps/run.ps1 -s my-settings.json -p song.json
        pwsh -File ai_scripts/ps/run.ps1 --no-audio --project broken.json

    `--no-audio` starts the UI without opening the audio device, which keeps
    hosted plug-ins unloaded - the safe way to inspect a project that crashes a
    plug-in or to debug plug-in loading interactively.

.EXAMPLE
    pwsh -File ai_scripts/ps/run.ps1 --debug
#>
[CmdletBinding(PositionalBinding = $false)]
param(
    # Arguments passed straight to the PulseForge binary.
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$AppArgs
)

$ErrorActionPreference = 'Stop'
& cargo run -- @AppArgs
exit $LASTEXITCODE
