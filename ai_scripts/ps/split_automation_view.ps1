$path = 'crates/pulseforge-app/src/views/automation.rs'
$lines = Get-Content $path

function Find-Index($pattern, $from) {
    for ($i = $from; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match $pattern) { return $i }
    }
    throw "not found: $pattern"
}

$A = Find-Index '// Indices can go stale' 0
$L = Find-Index 'Automate every continuous' 0
$B = Find-Index '^        let target = if self\.automation_effect_mode \{' 0
$C = Find-Index '^        if let Some\(target\) = target \{' 0
$D = Find-Index '^        ui\.separator\(\);' $C
$E = Find-Index '^        // Quick formula tool' 0
$F = Find-Index '^        ui\.separator\(\);' $E
$G = Find-Index '^        let mut changed = false;' 0
$H = Find-Index '^impl PulseForgeApp \{$' $C
$I = Find-Index 'pub\(crate\) fn add_formula_automation' $H
"anchors A=$A L=$L B=$B C=$C D=$D E=$E F=$F G=$G H=$H I=$I count=$($lines.Count)"

$clamp = $lines[$A..($L - 1)]
$selector = $lines[$B..($C - 2)]
$addLane = $lines[($C + 1)..($D - 3)] | ForEach-Object {
    if ($_ -match '^    ') { $_.Substring(4) } else { $_ }
}
$tracks = $lines[$G..($H - 4)]
$formulaTool = $lines[$E..($F - 2)]
$formulaBody = $lines[($I - 3)..($lines.Count - 3)]

$targetFile = @(
    '//! Picking the parameter that the automation window edits.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {',
    '    /// Keeps the remembered instrument, channel and slot indices inside the',
    '    /// current project, so opening the window after a removal cannot panic.',
    '    fn clamp_automation_indices(&mut self) {'
) + $clamp + @(
    '    }',
    '',
    '    /// Draws the instrument / insert-effect picker.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI.',
    '    ///',
    '    /// # Returns',
    '    ///',
    '    /// The selected [`ParameterTarget`], or `None` when the project has',
    '    /// nothing that could be automated yet.',
    '    fn automation_target_selector(&mut self, ui: &mut egui::Ui) -> Option<ParameterTarget> {'
) + $selector + @(
    '        target',
    '    }',
    '',
    '    /// Draws the button that turns the selected target into a fresh lane.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI.',
    '    /// - `target`: the parameter the new lane should automate.',
    '    fn automation_add_lane_button(&mut self, ui: &mut egui::Ui, target: ParameterTarget) {'
) + $addLane + @(
    '    }',
    '}',
    ''
)
Set-Content -Path 'crates/pulseforge-app/src/views/automation_target.rs' -Value $targetFile -Encoding utf8

$formulaFile = @(
    '//! The formula tool of the automation window.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {',
    '    /// Shows the formula editor, a live preview and the button that samples',
    '    /// the expression into a lane.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI.',
    '    fn automation_formula_tool(&mut self, ui: &mut egui::Ui) {'
) + $formulaTool + @(
    '    }',
    ''
) + $formulaBody + @(
    '}',
    ''
)
Set-Content -Path 'crates/pulseforge-app/src/views/automation_formula.rs' -Value $formulaFile -Encoding utf8

$tracksFile = @(
    '//! The editable list of automation lanes.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {',
    '    /// Draws one block per lane: name, target, clip range and the controls of',
    '    /// the selected modulation source.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI.',
    '    fn automation_track_list(&mut self, ui: &mut egui::Ui) {'
) + $tracks + @(
    '    }',
    '}',
    ''
)
Set-Content -Path 'crates/pulseforge-app/src/views/automation_tracks.rs' -Value $tracksFile -Encoding utf8

$newMain = @(
    '//! The automation window.',
    '//!',
    '//! The window is an assembly of three helpers that live in their own modules:',
    '//! [`automation_target`](super::automation_target) owns the target picker,',
    '//! [`automation_formula`](super::automation_formula) the formula tool and',
    '//! [`automation_tracks`](super::automation_tracks) the editable list of lanes.',
    '',
    'use crate::*;',
    '',
    'impl PulseForgeApp {',
    '    /// Draws the automation window.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI.',
    '    pub(crate) fn render_automation_window(&mut self, ui: &mut egui::Ui) {',
    '        self.clamp_automation_indices();',
    '        ui.label("Automate every continuous built-in instrument and insert-effect control.");',
    '        ui.horizontal(|ui| {',
    '            ui.selectable_value(&mut self.automation_effect_mode, false, "Instrument");',
    '            ui.selectable_value(&mut self.automation_effect_mode, true, "Insert FX");',
    '        });',
    '        if let Some(target) = self.automation_target_selector(ui) {',
    '            self.automation_add_lane_button(ui, target);',
    '        }',
    '        ui.separator();',
    '        self.automation_formula_tool(ui);',
    '        ui.separator();',
    '        self.automation_track_list(ui);',
    '    }',
    '}',
    ''
)
Set-Content -Path $path -Value $newMain -Encoding utf8

Get-ChildItem 'crates/pulseforge-app/src/views/automation*.rs' | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
