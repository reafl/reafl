param([string]$Path = "crates/reafl-widgets/src/knob.rs")

# Splits `Knob::show` into a body that also hands back the widget response, so
# the unified control can attach tooltips and menus to the same widget.
$lines = Get-Content -LiteralPath $Path
$out = @()
foreach ($line in $lines) {
    if ($line -match '^    pub fn show\(self, ui: &mut egui::Ui, value: &mut f32\) -> bool \{') {
        $out += '    pub fn show(self, ui: &mut egui::Ui, value: &mut f32) -> bool {'
        $out += '        self.show_response(ui, value).0'
        $out += '    }'
        $out += ''
        $out += '    /// Draws the knob and also returns its widget response.'
        $out += '    ///'
        $out += '    /// # Parameters'
        $out += '    ///'
        $out += '    /// - `ui`: the parent layout.'
        $out += '    /// - `value`: the value to edit, written back in place.'
        $out += '    ///'
        $out += '    /// # Returns'
        $out += '    ///'
        $out += '    /// Whether the value changed, and the response of the dial.'
        $out += '    pub fn show_response('
        $out += '        self,'
        $out += '        ui: &mut egui::Ui,'
        $out += '        value: &mut f32,'
        $out += '    ) -> (bool, egui::Response) {'
        continue
    }
    if ($line -match '^        let _ = editing;$') {
        $out += $line
        $out += '        (changed, response)'
        continue
    }
    if ($line -match '^        changed$') {
        continue
    }
    $out += $line
}
Set-Content -LiteralPath $Path -Value $out
Write-Host 'knob split'
