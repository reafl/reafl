param([string]$Root = "crates/pulseforge-widgets/src")

# Inserts the missing rustdoc lines above the public items that still lack them.
$docs = @(
    @{ File = 'anim.rs'; Anchor = 'pub fn new(value: f32) -> Self {'; Text = @(
        '/// Creates an eased value that starts settled at `value`.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `value`: the starting value.',
        '///',
        '/// # Returns',
        '///',
        '/// An animation already at rest.'
    ) },
    @{ File = 'anim.rs'; Anchor = 'pub fn set(&mut self, target: f32) {'; Text = @(
        '/// Sets the target the value eases toward.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `target`: the value to reach.'
    ) },
    @{ File = 'anim.rs'; Anchor = 'pub fn step(&mut self, dt: f32, enabled: bool, speed: f32) -> f32 {'; Text = @(
        '/// Advances toward the target by one frame.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `dt`: frame time in seconds.',
        '/// - `enabled`: when `false` the value snaps to its target, which is how',
        '///   the animations setting is honoured.',
        '/// - `speed`: multiplier on the stiffness.',
        '///',
        '/// # Returns',
        '///',
        '/// The eased value for this frame.'
    ) },
    @{ File = 'anim.rs'; Anchor = 'pub fn value(&self) -> f32 {'; Text = @(
        '/// The value as it stands this frame.',
        '///',
        '/// # Returns',
        '///',
        '/// The current, possibly still moving, value.'
    ) },
    @{ File = 'anim.rs'; Anchor = 'pub fn is_animating(&self) -> bool {'; Text = @(
        '/// True while the value is still chasing its target.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when another frame is needed to reach the target.'
    ) },
    @{ File = 'anim.rs'; Anchor = 'pub fn smoothstep(value: f32) -> f32 {'; Text = @(
        '/// Smoothstep easing for a normalised value.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `value`: progress in `0..=1`.',
        '///',
        '/// # Returns',
        '///',
        '/// The eased progress, with zero slope at both ends.'
    ) },
    @{ File = 'theme.rs'; Anchor = 'pub fn apply_theme(context: &egui::Context, theme: &settings::Theme) {'; Text = @(
        '/// Pushes a theme into egui visuals and stores it for the free drawing',
        '/// helpers.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `context`: the egui context to restyle.',
        '/// - `theme`: the colours and sizes to apply.'
    ) },
    @{ File = 'theme.rs'; Anchor = 'pub fn active_theme(context: &egui::Context) -> settings::Theme {'; Text = @(
        '/// The theme currently in force, for drawing code outside a widget tree.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `context`: the egui context holding the stored theme.',
        '///',
        '/// # Returns',
        '///',
        '/// The active theme, or the default one before [`apply_theme`] has run.'
    ) },
    @{ File = 'theme.rs'; Anchor = 'pub fn lane_color(index: usize) -> egui::Color32 {'; Text = @(
        '/// A stable accent colour for a track index.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `index`: the track number.',
        '///',
        '/// # Returns',
        '///',
        '/// One of the palette colours, cycling with the index.'
    ) },
    @{ File = 'theme.rs'; Anchor = 'pub fn color_lerp(start: egui::Color32, end: egui::Color32, amount: f32) -> egui::Color32 {'; Text = @(
        '/// Blends two colours.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `start`: colour at `0.0`.',
        '/// - `end`: colour at `1.0`.',
        '/// - `amount`: blend factor, clamped to `0..=1`.',
        '///',
        '/// # Returns',
        '///',
        '/// The interpolated colour.'
    ) },
    @{ File = 'theme.rs'; Anchor = 'pub fn point_segment_distance(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {'; Text = @(
        '/// Distance from a point to a line segment, used for cable hit testing.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `point`: the point to measure from.',
        '/// - `start`: first end of the segment.',
        '/// - `end`: second end of the segment.',
        '///',
        '/// # Returns',
        '///',
        '/// The shortest distance in points.'
    ) },
    @{ File = 'tooltip.rs'; Anchor = 'pub fn tooltip(ui: &egui::Ui, response: &egui::Response, text: &str) {'; Text = @(
        '/// Applies the configured tooltip policy to a widget.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the layout the widget lives in.',
        '/// - `response`: the widget to describe.',
        '/// - `text`: what to show when the pointer rests on it.'
    ) },
    @{ File = 'meter.rs'; Anchor = 'pub fn render_meter_sized(ui: &mut egui::Ui, level: f32, width: f32, height: f32) {'; Text = @(
        '/// Draws a segmented level meter.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `level`: peak level in `0..=1`.',
        '/// - `width`: meter width in points.',
        '/// - `height`: meter height in points.'
    ) },
    @{ File = 'lane_slider.rs'; Anchor = 'pub fn lane_parameter_slider('; Text = @(
        '/// Slider for one of an automation lane own parameters.',
        '///',
        '/// While a lane drives the value the slider sits at the engine value; a',
        '/// drag still writes through to the stored value.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `label`: caption shown beside the slider.',
        '/// - `value`: the parameter to edit.',
        '/// - `range`: inclusive bounds.',
        '/// - `target`: the automation target this control stands for.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when the value changed this frame.'
    ) },
    @{ File = 'fader.rs'; Anchor = 'pub fn fader('; Text = @(
        '/// Vertical dB fader whose travel matches the meter beside it.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `gain`: linear gain, edited in place.',
        '/// - `width`: fader width in points.',
        '/// - `height`: fader height in points.',
        '/// - `target`: automation target, or `None` for a plain fader.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when the gain changed this frame.'
    ) },
    @{ File = 'effect_parameters.rs'; Anchor = 'pub fn render_effect_parameters('; Text = @(
        '/// Draws the parameter knobs for one insert slot.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `slot`: the insert slot whose parameters are edited.',
        '/// - `channel_id`: mixer channel owning the slot.',
        '/// - `slot_id`: insert slot index.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when any parameter changed.'
    ) },
    @{ File = 'automation_bridge.rs'; Anchor = 'pub fn target_is_automated(ui: &egui::Ui, target: ParameterTarget) -> bool {'; Text = @(
        '/// True when a lane already drives `target`.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: any layout, used for its context data.',
        '/// - `target`: the parameter to check.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when a lane in the range covers the playhead.'
    ) },
    @{ File = 'automation_bridge.rs'; Anchor = 'pub fn target_live_value(ui: &egui::Ui, target: ParameterTarget) -> Option<f32> {'; Text = @(
        '/// The engine value a lane is currently applying.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: any layout, used for its context data.',
        '/// - `target`: the parameter to look up.',
        '///',
        '/// # Returns',
        '///',
        '/// The live value, or `None` when no lane is driving the target.'
    ) },
    @{ File = 'automation_bridge.rs'; Anchor = 'pub fn request_automation_lane(context: &egui::Context, target: ParameterTarget) {'; Text = @(
        '/// Queues a request to create a lane for `target`.',
        '///',
        '/// The editor picks the request up on its next frame, which keeps widgets',
        '/// free of any dependency on the application type.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `context`: the egui context to queue the request in.',
        '/// - `target`: the parameter to automate.'
    ) },
    @{ File = 'automation_bridge.rs'; Anchor = 'pub fn automation_context_menu(response: &egui::Response, label: &str, target: ParameterTarget) {'; Text = @(
        '/// Adds an "Add automation lane..." entry to a control context menu.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `response`: the control the menu belongs to.',
        '/// - `label`: text naming the control and its value.',
        '/// - `target`: the parameter a new lane would drive.'
    ) },
    @{ File = 'automation_bridge.rs'; Anchor = 'pub fn automation_lane_button(ui: &mut egui::Ui, label: &str, target: ParameterTarget) -> bool {'; Text = @(
        '/// A small button that creates a lane, for controls that already own a',
        '/// popup such as a combo box.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `label`: text naming the control.',
        '/// - `target`: the parameter a new lane would drive.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when the button was clicked this frame.'
    ) },
    @{ File = 'time_edit.rs'; Anchor = 'pub fn edit_time_value('; Text = @(
        '/// Edits a time value: the amount plus its unit.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `track_id`: lane id, used to keep the unit combo ids unique.',
        '/// - `label`: caption shown before the amount.',
        '/// - `value`: the time value to edit.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when the amount or the unit changed.'
    ) },
    @{ File = 'time_edit.rs'; Anchor = 'pub fn edit_time_value_automated('; Text = @(
        '/// Edits a time value whose amount is also an automation target.',
        '///',
        '/// # Parameters',
        '///',
        '/// - `ui`: the parent layout.',
        '/// - `track_id`: lane id, used to keep the unit combo ids unique.',
        '/// - `label`: caption shown before the amount.',
        '/// - `value`: the time value to edit.',
        '/// - `target`: automation target for the amount, if any.',
        '///',
        '/// # Returns',
        '///',
        '/// `true` when the amount or the unit changed.'
    ) }
)

foreach ($entry in $docs) {
    $path = Join-Path $Root $entry.File
    $lines = Get-Content -LiteralPath $path
    $index = -1
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i].Trim() -eq $entry.Anchor) { $index = $i; break }
    }
    if ($index -lt 0) { Write-Host "anchor not found in $($entry.File): $($entry.Anchor)"; continue }
    if ($index -gt 0 -and $lines[$index - 1] -match '^\s*///') { continue }
    $before = if ($index -gt 0) { $lines[0..($index - 1)] } else { @() }
    $after = $lines[$index..($lines.Count - 1)]
    Set-Content -LiteralPath $path -Value ($before + $entry.Text + $after)
}
Write-Host 'docs inserted'
