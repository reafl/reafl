$path = 'crates/reafl-app/src/views/timeline.rs'
$lines = Get-Content $path

function Find-Index($pattern, $from) {
    for ($i = $from; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match $pattern) { return $i }
    }
    throw "not found: $pattern"
}

$P = Find-Index '^        painter\.text\($' 0
$R = Find-Index '^        let lane_count = self\.project\.lanes\.len\(\);' 0
$G = Find-Index '^        for bar in 0\.\.=bars_visible \{' 0
$I = Find-Index '^        let \($' 0
"anchors P=$P R=$R G=$G I=$I count=$($lines.Count)"

$ruler = $lines[$P..($R - 1)]
$rowBlock = $lines[$R..($G - 1)]
$grid = $lines[$G..($I - 1)]
$input = $lines[$I..($lines.Count - 3)]

$rowBlock = $rowBlock | Where-Object {
    $_ -notmatch '^        let rows = arrangement_layout\(&self\.project\);$' -and
    $_ -notmatch '^        let row_index_at = '
}

function Convert-Block($block) {
    $out = foreach ($line in $block) {
        $l = $line
        $l = $l -replace 'timeline_rect', 'geo.rect'
        $l = $l -replace '(?<![\w.])left\b', 'geo.left'
        $l = $l -replace '\bgrid_top\b', 'geo.grid_top'
        $l = $l -replace '\bstep_width\b', 'geo.step_width'
        $l = $l -replace '(?<![\w.])start_step\b', 'geo.start_step'
        $l = $l -replace '\bbar_width\b', 'geo.bar_width'
        $l = $l -replace '\bsteps_per_bar\b', 'geo.steps_per_bar'
        $l = $l -replace '\bbars_visible\b', 'geo.bars_visible'
        $l = $l -replace '\bview_end_step\b', 'geo.view_end_step'
        $l = $l -replace '\bfirst_lane\b', 'geo.first_lane'
        $l = $l -replace '\bfirst_bar\b', 'geo.first_bar'
        $l = $l -replace '\bheader_height\b', 'geo.header_height'
        $l = $l -replace 'row_index_at\(', 'arrangement_row_at(rows, '
        $l = $l -replace '&painter', 'painter'
        $l
    }
    return $out
}

$rulerOut = Convert-Block $ruler
$rowsOut = Convert-Block $rowBlock
$gridOut = Convert-Block $grid
$inputOut = Convert-Block $input

$rowsFile = @(
    '//! Painting the arrangement rows: track headers, power switches and clips.',
    '',
    'use crate::*;',
    '',
    'impl ReaflApp {',
    '    /// Draws every visible arrangement row and applies the context-menu and',
    '    /// duplication requests the interactions collected.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI, used to register the per-row interactions.',
    '    /// - `context`: the frame context, for pointer deltas and cursor icons.',
    '    /// - `painter`: the painter of the timeline surface.',
    '    /// - `geo`: the geometry of the current frame.',
    '    /// - `rows`: the arrangement layout for the current project.',
    '    pub(crate) fn paint_timeline_rows(',
    '        &mut self,',
    '        ui: &mut egui::Ui,',
    '        context: &egui::Context,',
    '        painter: &egui::Painter,',
    '        geo: TimelineGeometry,',
    '        rows: &[RowSpan],',
    '    ) {'
) + $rowsOut + @(
    '    }',
    '}',
    ''
)
Set-Content -Path 'crates/reafl-app/src/views/timeline_rows.rs' -Value $rowsFile -Encoding utf8

$paintFile = @(
    '//! Painting the timeline ruler and the bar/beat grid.',
    '',
    'use crate::*;',
    '',
    'impl ReaflApp {',
    '    /// Draws the bar ruler above the rows, including the loop marker.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `painter`: the painter of the timeline surface.',
    '    /// - `geo`: the geometry of the current frame.',
    '    pub(crate) fn paint_timeline_ruler(&self, painter: &egui::Painter, geo: TimelineGeometry) {'
) + $rulerOut + @(
    '    }',
    '',
    '    /// Draws the bar and beat lines, the row separators, the playhead and the',
    '    /// empty space below the last row.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `painter`: the painter of the timeline surface.',
    '    /// - `geo`: the geometry of the current frame.',
    '    /// - `rows`: the arrangement layout for the current project.',
    '    pub(crate) fn paint_timeline_grid(',
    '        &self,',
    '        painter: &egui::Painter,',
    '        geo: TimelineGeometry,',
    '        rows: &[RowSpan],',
    '    ) {'
) + $gridOut + @(
    '    }',
    '}',
    ''
)
Set-Content -Path 'crates/reafl-app/src/views/timeline_paint.rs' -Value $paintFile -Encoding utf8

$inputFile = @(
    '//! Pointer and wheel interaction of the arrangement timeline.',
    '',
    'use crate::*;',
    '',
    'impl ReaflApp {',
    '    /// Turns clicks, drags and the wheel over the timeline into selection,',
    '    /// clip gestures, painting, slicing and panning.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI, used for the tool cursor.',
    '    /// - `context`: the frame context, for the raw pointer state.',
    '    /// - `painter`: the painter of the timeline surface.',
    '    /// - `response`: the response of the timeline surface.',
    '    /// - `geo`: the geometry of the current frame.',
    '    /// - `rows`: the arrangement layout for the current project.',
    '    pub(crate) fn handle_timeline_input(',
    '        &mut self,',
    '        ui: &mut egui::Ui,',
    '        context: &egui::Context,',
    '        painter: &egui::Painter,',
    '        response: egui::Response,',
    '        geo: TimelineGeometry,',
    '        rows: &[RowSpan],',
    '    ) {'
) + $inputOut + @(
    '    }',
    '}',
    ''
)
Set-Content -Path 'crates/reafl-app/src/views/timeline_input.rs' -Value $inputFile -Encoding utf8

$mainFile = @(
    '//! The arrangement timeline.',
    '//!',
    '//! One frame of the timeline is built in three passes that live in their own',
    '//! modules: [`timeline_paint`](super::timeline_paint) paints the ruler and the',
    '//! grid, [`timeline_rows`](super::timeline_rows) the track headers and clips, and',
    '//! [`timeline_input`](super::timeline_input) turns pointer input into edits.',
    '',
    'use crate::*;',
    '',
    '/// Geometry of the arrangement timeline for one frame.',
    '///',
    '/// Every pass needs the same origin, zoom and view window, so they are',
    '/// computed once and handed around by value.',
    '#[derive(Clone, Copy)]',
    'pub(crate) struct TimelineGeometry {',
    '    /// The full timeline surface.',
    '    pub(crate) rect: egui::Rect,',
    '    /// Top edge of the row area, below the ruler.',
    '    pub(crate) grid_top: f32,',
    '    /// Left edge of the first bar.',
    '    pub(crate) left: f32,',
    '    /// Height of the bar ruler.',
    '    pub(crate) header_height: f32,',
    '    /// First step that is drawn.',
    '    pub(crate) start_step: u64,',
    '    /// Width of one step in pixels.',
    '    pub(crate) step_width: f32,',
    '    /// Width of one bar in pixels.',
    '    pub(crate) bar_width: f32,',
    '    /// Steps per bar according to the time signature.',
    '    pub(crate) steps_per_bar: u64,',
    '    /// Number of bars that fit on screen.',
    '    pub(crate) bars_visible: u64,',
    '    /// First bar that is drawn.',
    '    pub(crate) first_bar: u64,',
    '    /// One past the last step that is drawn.',
    '    pub(crate) view_end_step: u64,',
    '    /// First arrangement row that is drawn.',
    '    pub(crate) first_lane: usize,',
    '}',
    '',
    'impl ReaflApp {',
    '    /// Draws the whole arrangement timeline.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `ui`: the parent UI.',
    '    /// - `context`: the frame context.',
    '    pub(crate) fn render_timeline(&mut self, ui: &mut egui::Ui, context: &egui::Context) {',
    '        let (rect, response) =',
    '            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());',
    '        let painter = ui.painter_at(rect);',
    '        painter.rect_filled(rect, 8.0, egui::Color32::from_gray(24));',
    '        let geo = self.timeline_geometry(rect);',
    '        let rows = arrangement_layout(&self.project);',
    '        self.paint_timeline_ruler(&painter, geo);',
    '        self.paint_timeline_rows(ui, context, &painter, geo, &rows);',
    '        self.paint_timeline_grid(&painter, geo, &rows);',
    '        self.handle_timeline_input(ui, context, &painter, response, geo, &rows);',
    '    }',
    '',
    '    /// Measures the timeline for the given surface.',
    '    ///',
    '    /// # Parameters',
    '    ///',
    '    /// - `rect`: the area the timeline covers.',
    '    ///',
    '    /// # Returns',
    '    ///',
    '    /// The geometry shared by the painting and interaction passes.',
    '    fn timeline_geometry(&self, rect: egui::Rect) -> TimelineGeometry {',
    '        let steps_per_bar = self.project.time_signature.steps_per_bar();',
    '        let step_width = BAR_WIDTH * self.settings.zoom / steps_per_bar as f32;',
    '        let bar_width = BAR_WIDTH * self.settings.zoom;',
    '        let bars_visible = ((rect.width() - LANE_LABEL_WIDTH) / bar_width).ceil() as u64 + 1;',
    '        let first_bar = self.arrangement_pan_steps / steps_per_bar;',
    '        let start_step = first_bar.saturating_mul(steps_per_bar);',
    '        let header_height = 30.0;',
    '        // Rows have independent heights, so the visible count is discovered',
    '        // after the layout is built instead of being divided by a constant.',
    '        let view_end_step =',
    '            start_step.saturating_add(bars_visible.saturating_mul(steps_per_bar));',
    '        TimelineGeometry {',
    '            rect,',
    '            grid_top: rect.top() + header_height,',
    '            left: rect.left() + LANE_LABEL_WIDTH,',
    '            header_height,',
    '            start_step,',
    '            step_width,',
    '            bar_width,',
    '            steps_per_bar,',
    '            bars_visible,',
    '            first_bar,',
    '            view_end_step,',
    '            first_lane: self.arrangement_pan_lanes,',
    '        }',
    '    }',
    '}',
    ''
)
Set-Content -Path $path -Value $mainFile -Encoding utf8

Get-ChildItem 'crates/reafl-app/src/views/timeline*.rs' | ForEach-Object { "{0,5} {1}" -f (Get-Content $_.FullName).Count, $_.Name }
