param(
    [string]$Source = "crates/pulseforge-app/src/lib.rs",
    [string]$TargetDir = "crates/pulseforge-app/src"
)

# Moves a contiguous block of top-level items out of the monolith and into its
# own module, making the items visible to the parent module.
function Cut-Module {
    param(
        [string]$File,
        [int]$StartLine,
        [int]$EndLine,
        [string]$Name,
        [string]$Title
    )
    $lines = Get-Content -LiteralPath $File
    $chunk = $lines[($StartLine - 1)..($EndLine - 1)]
    $body = $chunk | ForEach-Object {
        $_ -replace '^(fn |struct |enum |const |static )', 'pub(crate) $1'
    }
    $header = @("//! $Title", "", "use super::*;", "")
    $path = Join-Path $TargetDir "$Name.rs"
    Set-Content -LiteralPath $path -Value ($header + $body)
    $before = if ($StartLine -gt 1) { $lines[0..($StartLine - 2)] } else { @() }
    $after = if ($EndLine -lt $lines.Count) { $lines[$EndLine..($lines.Count - 1)] } else { @() }
    Set-Content -LiteralPath $File -Value ($before + $after)
    Write-Host "extracted $Name.rs ($($chunk.Count) lines)"
}

Cut-Module $Source 12104 12317 labels "Human-readable names and option lists."
Cut-Module $Source 12064 12102 fader_math "The fader's dB scale and its silence snap."
Cut-Module $Source 12017 12062 snap "Quantisation helpers for the editors."
Cut-Module $Source 11976 12014 tooltip "Small hover and text helpers."
Cut-Module $Source 11910 11975 presets "Mixer chain presets stored on disk."
Cut-Module $Source 11666 11909 icons "Vector glyphs and the icon buttons built from them."
Cut-Module $Source 11590 11665 geometry "Pointer geometry and small palette helpers."
Cut-Module $Source 11505 11589 theme "Colour tokens, theme application and colour maths."
