<#
.SYNOPSIS
    Repairs source files written with the ANSI default encoding.

.DESCRIPTION
    An earlier codemod used `Set-Content` without an encoding, which makes
    Windows PowerShell write the ANSI code page. Any file containing non-ASCII
    characters (em dashes, arrows, accented labels in doc comments) then became
    invalid UTF-8. This script walks the given folders, detects files that are
    not valid UTF-8, re-reads them with the ANSI code page, and rewrites them as
    UTF-8 without a byte order mark.

    It is safe to run repeatedly: files that already decode as UTF-8 are left
    untouched.

.EXAMPLE
    pwsh -File ai_scripts/ps/repair_utf8.ps1 -Roots crates/pulseforge-core/src,crates/pulseforge-app/src
#>
[CmdletBinding()]
param(
    [string[]] $Roots = @(
        'crates/pulseforge-core/src',
        'crates/pulseforge-app/src',
        'crates/pulseforge-widgets/src'
    )
)

$ErrorActionPreference = 'Stop'

$utf8 = New-Object System.Text.UTF8Encoding($false)
$strict = New-Object System.Text.UTF8Encoding($false, $true)
$ansi = [System.Text.Encoding]::Default

$repaired = 0
$checked = 0
foreach ($root in $Roots) {
    foreach ($file in Get-ChildItem $root -Recurse -Filter *.rs) {
        $checked++
        $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
        try {
            [void]$strict.GetString($bytes)
            continue
        } catch {
            $text = $ansi.GetString($bytes)
            [System.IO.File]::WriteAllText($file.FullName, $text, $utf8)
            $repaired++
        }
    }
}

"checked $checked file(s), repaired $repaired"
