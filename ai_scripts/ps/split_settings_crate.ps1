$src = 'crates/reafl-settings/src/settings.rs'
$dir = 'crates/reafl-settings/src/settings'
if (Test-Path $dir) { throw "already exists: $dir" }
New-Item -ItemType Directory -Path $dir | Out-Null
$lines = Get-Content $src

function Find-Index($pattern, $from) {
    for ($i = $from; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match $pattern) { return $i }
    }
    throw "not found: $pattern"
}
function Doc-Start($index) {
    $i = $index
    while ($i -gt 0 -and ($lines[$i - 1].TrimStart().StartsWith('///') -or $lines[$i - 1].TrimStart().StartsWith('#['))) { $i-- }
    return $i
}

$A = Doc-Start (Find-Index '^pub enum ShortcutAction \{' 0)
$B = Doc-Start (Find-Index '^pub struct Settings \{' 0)
$C = Doc-Start (Find-Index '^pub struct Theme \{' 0)
$E = Doc-Start (Find-Index '^impl Default for Settings \{' 0)
$F = Doc-Start (Find-Index '^pub fn default_shortcuts\(\)' 0)
$G = Doc-Start (Find-Index '^impl Settings \{' 0)
$H = Doc-Start (Find-Index '^#\[cfg\(test\)\]' 0)
"anchors A=$A B=$B C=$C E=$E F=$F G=$G H=$H count=$($lines.Count)"

$shortcuts = $lines[$A..($B - 1)] + @('') + $lines[$F..($G - 1)]
$appearance = $lines[$C..($E - 1)]
$core = $lines[$B..($C - 1)] + @('') + $lines[$E..($F - 1)] + @('') + $lines[$G..($H - 1)]
$tests = $lines[$H..($lines.Count - 1)]

$shortcutFile = @(
    '//! Key bindings: the bindable commands and the chords assigned to them.',
    '',
    'use serde::{Deserialize, Serialize};',
    ''
) + $shortcuts + @('')
Set-Content -Path "$dir/shortcuts.rs" -Value $shortcutFile -Encoding utf8

$appearanceFile = @(
    '//! Appearance: colour theme and tooltip behaviour.',
    '',
    'use serde::{Deserialize, Serialize};',
    ''
) + $appearance + @('')
Set-Content -Path "$dir/appearance.rs" -Value $appearanceFile -Encoding utf8

$testsFile = @(
    '//! Unit tests for the settings model.',
    ''
) + $tests[1..($tests.Count - 1)] + @('')
Set-Content -Path "$dir/tests.rs" -Value $testsFile -Encoding utf8

$mainFile = @(
    '//! Persistent application settings.',
    '//!',
    '//! Everything the settings window edits lives here and is stored in',
    '//! `settings.json`. Keyboard bindings are data, not code: [`ShortcutAction`]',
    '//! lists every command, and [`Settings::bind`] assigns a chord to one, so a new',
    '//! command only needs an enum variant plus a handler.',
    '//!',
    '//! The value types live in their own modules: [`shortcuts`] holds the bindable',
    '//! commands and chords, [`appearance`] the theme and tooltip options.',
    '',
    'mod appearance;',
    'mod shortcuts;',
    '',
    '#[cfg(test)]',
    'mod tests;',
    '',
    'pub use appearance::*;',
    'pub use shortcuts::*;',
    '',
    'use std::fs;',
    '',
    'use serde::{Deserialize, Serialize};',
    '',
    '/// File name used inside the working directory.',
    'pub const SETTINGS_FILE: &str = "settings.json";',
    ''
) + $core + @('', '#[cfg(test)]', 'mod tests_unused;', '')
# drop the placeholder tail we just added; tests are registered above
$mainFile = $mainFile[0..($mainFile.Count - 4)]
Set-Content -Path "$dir/mod.rs" -Value $mainFile -Encoding utf8

Remove-Item $src
Get-ChildItem $dir | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
