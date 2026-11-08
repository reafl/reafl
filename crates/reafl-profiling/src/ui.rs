//! The in-editor debug panel.
//!
//! One window with four sections: live machine/engine numbers, two overview
//! strips (UI frame time, audio DSP load), a flame graph per channel, and a
//! table of hosted plug-ins. All drawing is plain egui shapes, so the panel
//! needs no plot dependency, and the strip reductions can run on a compute
//! shader (feature `gpu`) with an identical CPU fallback.

use std::collections::VecDeque;

use egui::{Color32, Rect, RichText, Stroke, Vec2};

use crate::{
    flame::{ChannelFlame, FlameNode},
    plugin::{PluginDebug, PluginSample},
    span::{Channel, Profiler},
};

#[cfg(feature = "gpu")]
use crate::gpu::{Column, GpuHistogram};

#[cfg(feature = "system")]
use crate::system::{SystemMonitor, format_bytes};

/// Live audio-engine numbers the app feeds in once per frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioDebug {
    pub sample_rate: u32,
    /// Frames per output callback, when the device reports it.
    pub block_frames: u32,
    /// Callback work as a fraction of the callback period (1.0 = real time).
    pub dsp_load: f32,
    pub playing: bool,
}

/// Everything the panel reads. `plugins` is mutable because sampling advances
/// its rate deltas.
pub struct DebugWorld<'a> {
    pub profiler: Option<&'a Profiler>,
    #[cfg(feature = "system")]
    pub system: Option<&'a SystemMonitor>,
    pub plugins: &'a mut PluginDebug,
    pub audio: AudioDebug,
}

impl AudioDebug {
    /// Callback load as a percentage of real time.
    pub fn load_percent(&self) -> f32 {
        self.dsp_load * 100.0
    }
}

/// Number of samples kept for each overview strip.
const STRIP_SAMPLES: usize = 600;

/// Height of one flame graph row.
const ROW_HEIGHT: f32 = 18.0;

/// Panel state: window visibility plus the small graphs' history.
pub struct DebugPanel {
    pub open: bool,
    channel: Channel,
    frame_ms: VecDeque<f32>,
    dsp_load: VecDeque<f32>,
    plugins: Vec<PluginSample>,
    show_self_time: bool,
    peak_load: f32,
    #[cfg(feature = "gpu")]
    gpu: GpuHistogram,
    #[cfg(feature = "gpu")]
    use_gpu: bool,
    #[cfg(feature = "gpu")]
    gpu_tick: u32,
}

impl Default for DebugPanel {
    fn default() -> Self {
        Self::new(false)
    }
}

impl DebugPanel {
    pub fn new(open: bool) -> Self {
        Self {
            open,
            channel: Channel::Ui,
            frame_ms: VecDeque::with_capacity(STRIP_SAMPLES),
            dsp_load: VecDeque::with_capacity(STRIP_SAMPLES),
            plugins: Vec::new(),
            show_self_time: false,
            peak_load: 0.0,
            #[cfg(feature = "gpu")]
            gpu: GpuHistogram::new(),
            #[cfg(feature = "gpu")]
            use_gpu: true,
            #[cfg(feature = "gpu")]
            gpu_tick: 0,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Drops the collected history; call after resetting the profiler.
    pub fn clear(&mut self) {
        self.frame_ms.clear();
        self.dsp_load.clear();
        self.plugins.clear();
        self.peak_load = 0.0;
    }

    /// Draws the window when open. Call once per frame.
    pub fn show(&mut self, context: &egui::Context, world: &mut DebugWorld<'_>) {
        // History is collected even while closed so the graphs are populated
        // the moment the panel opens, at a negligible cost.
        let frame_ms = context.input(|input| input.stable_dt * 1000.0);
        push_capped(&mut self.frame_ms, frame_ms);
        push_capped(&mut self.dsp_load, world.audio.dsp_load);
        self.peak_load = self.peak_load.max(world.audio.dsp_load);
        self.plugins = world.plugins.sample();

        if !self.open {
            return;
        }
        context.request_repaint();
        let mut open = self.open;
        egui::Window::new("Debug · Profiler")
            .id(egui::Id::new("reafl_debug_panel"))
            .open(&mut open)
            .default_size([880.0, 620.0])
            .min_size([560.0, 320.0])
            .resizable(true)
            .show(context, |ui| {
                self.header(ui, world);
                ui.separator();
                self.metrics(ui, world);
                ui.separator();
                self.graphs(ui);
                ui.separator();
                self.flame(ui, world);
                ui.separator();
                plugin_table(ui, &self.plugins);
            });
        self.open = open;
    }

    fn header(&mut self, ui: &mut egui::Ui, world: &mut DebugWorld<'_>) {
        ui.horizontal_wrapped(|ui| {
            if let Some(profiler) = world.profiler {
                let mut enabled = profiler.enabled();
                if ui.checkbox(&mut enabled, "Profile").changed() {
                    profiler.set_enabled(enabled);
                }
                let mut seconds = profiler.window_ms() as f32 / 1000.0;
                if ui
                    .add(
                        egui::Slider::new(&mut seconds, 0.5..=30.0)
                            .suffix(" s")
                            .text("window"),
                    )
                    .changed()
                {
                    profiler.set_window_ms((seconds * 1000.0) as u64);
                }
                ui.checkbox(&mut self.show_self_time, "exclusive time");
                if ui.button("Clear").clicked() {
                    self.clear();
                }
                ui.label(
                    RichText::new(format!("{} span(s) dropped", profiler.dropped()))
                        .small()
                        .weak(),
                );
            } else {
                ui.label(RichText::new("profiler not installed").weak());
            }
            #[cfg(feature = "gpu")]
            {
                ui.separator();
                if ui.checkbox(&mut self.use_gpu, "GPU reduction").changed() {
                    // A fresh instance re-probes the driver on the next paint.
                    self.gpu = GpuHistogram::new();
                }
                let status = if !self.use_gpu {
                    "off".to_owned()
                } else if self.gpu.available() {
                    "compute shader".to_owned()
                } else if let Some(error) = self.gpu.error() {
                    format!("CPU fallback ({error})")
                } else {
                    "CPU fallback".to_owned()
                };
                ui.label(RichText::new(status).small().weak());
            }
        });
    }

    fn metrics(&mut self, ui: &mut egui::Ui, world: &mut DebugWorld<'_>) {
        let frame_ms = self.frame_ms.back().copied().unwrap_or(0.0);
        let worst_ms = self.frame_ms.iter().copied().fold(0.0_f32, f32::max);
        let mut items: Vec<(&str, String)> = vec![
            ("UI frame", format!("{frame_ms:.2} ms")),
            (
                "FPS",
                format!(
                    "{:.0}",
                    if frame_ms > 0.0 {
                        1000.0 / frame_ms
                    } else {
                        0.0
                    }
                ),
            ),
            ("Worst frame", format!("{worst_ms:.1} ms")),
            (
                "Transport",
                if world.audio.playing {
                    "playing"
                } else {
                    "stopped"
                }
                .to_owned(),
            ),
            ("Audio load", format!("{:.1} %", world.audio.load_percent())),
            ("Audio peak", format!("{:.1} %", self.peak_load * 100.0)),
            (
                "Format",
                format!(
                    "{} Hz · {} frames",
                    world.audio.sample_rate, world.audio.block_frames
                ),
            ),
        ];
        #[cfg(feature = "system")]
        if let Some(stats) = world.system.map(SystemMonitor::stats) {
            items.push(("CPU", format!("{:.0} %", stats.cpu_percent)));
            items.push((
                "RAM",
                format!(
                    "{} / {}",
                    format_bytes(stats.used_ram),
                    format_bytes(stats.total_ram)
                ),
            ));
            items.push(("Process RAM", format_bytes(stats.process_ram)));
            if stats.threads > 0 {
                items.push(("Threads", stats.threads.to_string()));
            }
        }
        egui::Grid::new("reafl_debug_metrics")
            .num_columns(4)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                for pair in items.chunks(2) {
                    for (label, value) in pair {
                        ui.label(RichText::new(*label).small().weak());
                        ui.label(RichText::new(value).monospace());
                    }
                    if pair.len() == 1 {
                        // Keep the grid aligned when the last row is short.
                        ui.label("");
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }

    fn graphs(&mut self, ui: &mut egui::Ui) {
        let frames: Vec<f32> = self.frame_ms.iter().copied().collect();
        let loads: Vec<f32> = self.dsp_load.iter().map(|load| load * 100.0).collect();
        #[cfg(feature = "gpu")]
        let gpu = {
            // One dispatch every fourth frame is plenty for a 600-sample strip
            // and avoids a read-back stall on every frame.
            self.gpu_tick = self.gpu_tick.wrapping_add(1);
            (self.use_gpu && self.gpu_tick % 4 == 0).then_some(&self.gpu)
        };
        #[cfg(not(feature = "gpu"))]
        let gpu = None;

        ui.label(RichText::new("UI frame time (ms)").small().weak());
        strip(
            ui,
            &frames,
            33.0,
            54.0,
            Color32::from_rgb(112, 208, 190),
            gpu,
        );
        ui.label(RichText::new("Audio DSP load (%)").small().weak());
        strip(
            ui,
            &loads,
            100.0,
            54.0,
            Color32::from_rgb(226, 168, 82),
            gpu,
        );
    }

    fn flame(&mut self, ui: &mut egui::Ui, world: &mut DebugWorld<'_>) {
        ui.horizontal(|ui| {
            for channel in Channel::ALL {
                ui.selectable_value(&mut self.channel, channel, channel.label());
            }
        });
        let Some(profiler) = world.profiler else {
            ui.label("Install the profiler to record spans.");
            return;
        };
        let snapshot = profiler.snapshot();
        let Some(flame) = snapshot.channel(self.channel) else {
            ui.label(RichText::new("no spans recorded in this window").weak());
            return;
        };
        ui.label(
            RichText::new(format!(
                "{} node(s) · {:.1} ms total · {:.1} s window",
                count_nodes(&flame.roots),
                flame.total_ns as f32 / 1e6,
                snapshot.window_seconds
            ))
            .small()
            .weak(),
        );
        draw_flame(ui, flame, self.show_self_time);
    }
}

/// Adds a value to a capped ring buffer.
fn push_capped(buffer: &mut VecDeque<f32>, value: f32) {
    if buffer.len() == STRIP_SAMPLES {
        buffer.pop_front();
    }
    buffer.push_back(value);
}

fn count_nodes(roots: &[FlameNode]) -> usize {
    let mut list = Vec::new();
    for root in roots {
        root.walk(0, &mut list);
    }
    list.len()
}

/// Reduced column shape shared by the GPU and CPU paths.
#[cfg(feature = "gpu")]
type StripColumn = Column;

#[cfg(not(feature = "gpu"))]
#[derive(Clone, Copy, Default)]
struct StripColumn {
    min: f32,
    max: f32,
    mean: f32,
}

/// Draws an overview strip: one vertical min/max bar per two pixels plus a mean
/// line, scaled to `max_value`.
fn strip(
    ui: &mut egui::Ui,
    values: &[f32],
    max_value: f32,
    height: f32,
    color: Color32,
    #[cfg(feature = "gpu")] gpu: Option<&GpuHistogram>,
    #[cfg(not(feature = "gpu"))] _gpu: Option<()>,
) {
    let width = ui.available_width().max(64.0);
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    if values.len() < 2 {
        return;
    }
    let columns = (rect.width() as usize / 2).max(1);
    let samples = resample(values, STRIP_SAMPLES);

    #[cfg(feature = "gpu")]
    if let Some(gpu) = gpu {
        gpu.paint(&painter, rect, &samples, columns);
    }

    let computed: Vec<StripColumn> = {
        #[cfg(feature = "gpu")]
        {
            let cached = gpu.map(GpuHistogram::result).unwrap_or_default();
            if cached.len() == columns {
                cached
            } else {
                crate::gpu::columns(&samples, columns)
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            cpu_columns(&samples, columns)
        }
    };
    let scale = if max_value > 0.0 { max_value } else { 1.0 };
    let mut line = Vec::with_capacity(computed.len());
    for (index, column) in computed.iter().enumerate() {
        let x = rect.left() + (index as f32 + 0.5) * rect.width() / computed.len() as f32;
        let top = rect.bottom() - (column.max / scale).clamp(0.0, 1.0) * rect.height();
        let bottom = rect.bottom() - (column.min / scale).clamp(0.0, 1.0) * rect.height();
        painter.line_segment(
            [egui::pos2(x, top), egui::pos2(x, bottom.max(top + 1.0))],
            Stroke::new(1.0, color.gamma_multiply(0.55)),
        );
        line.push(egui::pos2(
            x,
            rect.bottom() - (column.mean / scale).clamp(0.0, 1.0) * rect.height(),
        ));
    }
    painter.add(egui::Shape::line(line, Stroke::new(1.2, color)));
}

/// Nearest-sample resampling; enough for an overview strip.
fn resample(values: &[f32], max_len: usize) -> Vec<f32> {
    if values.len() <= max_len {
        return values.to_vec();
    }
    let step = values.len() as f32 / max_len as f32;
    (0..max_len)
        .map(|index| values[((index as f32 * step) as usize).min(values.len() - 1)])
        .collect()
}

/// CPU column reduction used when the GPU path is off or unavailable.
#[cfg(not(feature = "gpu"))]
fn cpu_columns(samples: &[f32], columns: usize) -> Vec<StripColumn> {
    (0..columns)
        .map(|column| {
            let begin = samples.len() * column / columns;
            let end = (samples.len() * (column + 1) / columns).max(begin);
            let slice = &samples[begin..end];
            if slice.is_empty() {
                return StripColumn::default();
            }
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            let mut sum = 0.0;
            for &value in slice {
                min = min.min(value);
                max = max.max(value);
                sum += value;
            }
            StripColumn {
                min,
                max,
                mean: sum / slice.len() as f32,
            }
        })
        .collect()
}

fn draw_flame(ui: &mut egui::Ui, flame: &ChannelFlame, exclusive: bool) {
    let mut flat: Vec<(u16, &FlameNode)> = Vec::new();
    for root in &flame.roots {
        root.walk(0, &mut flat);
    }
    if flat.is_empty() {
        return;
    }
    let depth = flat.iter().map(|(depth, _)| *depth).max().unwrap_or(0) as f32 + 1.0;
    let width = ui.available_width().max(64.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, (depth * ROW_HEIGHT).min(420.0)),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let total = flame.total_ns.max(1);
    let hover_position = response.hover_pos();
    let mut hovered: Option<&FlameNode> = None;

    // Preorder layout: a node starts where its previous sibling ended, and each
    // depth keeps its own cursor so children line up under their parent.
    let mut cursor: Vec<f32> = Vec::new();
    for (depth, node) in flat {
        let depth = usize::from(depth);
        let x = cursor.get(depth).copied().unwrap_or(rect.left());
        let node_width = node.fraction(total) * rect.width();
        cursor.truncate(depth + 1);
        cursor.push(x + node_width);
        if node_width < 1.0 {
            continue;
        }
        let bar = Rect::from_min_size(
            egui::pos2(x, rect.top() + depth as f32 * ROW_HEIGHT),
            Vec2::new((node_width - 1.0).max(1.0), ROW_HEIGHT - 2.0),
        );
        painter.rect_filled(bar, 2.0, depth_color(depth));
        if node_width > 46.0 {
            painter.text(
                bar.left_top() + Vec2::new(4.0, 3.0),
                egui::Align2::LEFT_TOP,
                node.name,
                egui::FontId::proportional(10.0),
                Color32::from_rgb(16, 18, 22),
            );
        }
        if hover_position.is_some_and(|position| bar.contains(position)) {
            hovered = Some(node);
        }
    }

    if let Some(node) = hovered {
        response.on_hover_ui(|ui| {
            ui.label(RichText::new(node.name).strong());
            ui.label(format!("inclusive {:.3} ms", node.total_ns as f32 / 1e6));
            ui.label(format!(
                "{} {:.3} ms",
                if exclusive { "exclusive" } else { "self" },
                node.self_ns as f32 / 1e6
            ));
            ui.label(format!("{} call(s)", node.calls));
        });
    }
}

fn depth_color(depth: usize) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(122, 198, 184),
        Color32::from_rgb(150, 178, 214),
        Color32::from_rgb(186, 168, 214),
        Color32::from_rgb(214, 178, 148),
        Color32::from_rgb(214, 200, 140),
        Color32::from_rgb(160, 208, 150),
        Color32::from_rgb(198, 160, 188),
        Color32::from_rgb(170, 170, 170),
    ];
    PALETTE[depth % PALETTE.len()]
}

fn plugin_table(ui: &mut egui::Ui, plugins: &[PluginSample]) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Hosted plug-ins").strong());
        ui.label(
            RichText::new(format!("{} loaded", plugins.len()))
                .small()
                .weak(),
        );
    });
    if plugins.is_empty() {
        ui.label(RichText::new("no hosted VST3/CLAP slots").weak());
        return;
    }
    egui::Grid::new("reafl_debug_plugins")
        .num_columns(6)
        .striped(true)
        .spacing([14.0, 3.0])
        .show(ui, |ui| {
            for heading in ["Plug-in", "Kind", "CPU", "Mean", "Peak", "Errors"] {
                ui.label(RichText::new(heading).small().strong());
            }
            ui.end_row();
            for plugin in plugins {
                ui.label(plugin.name.clone()).on_hover_text(format!(
                    "{} ({})",
                    plugin.path,
                    plugin.kind.label()
                ));
                ui.label(plugin.kind.label());
                ui.label(format!("{:.1} %", plugin.cpu_percent));
                ui.label(format!("{:.0} µs", plugin.mean_micros));
                ui.label(format!("{:.0} µs", plugin.peak_micros));
                if plugin.errors == 0 {
                    ui.label("0");
                } else {
                    let text = plugin
                        .last_error
                        .clone()
                        .unwrap_or_else(|| "no message".to_owned());
                    ui.colored_label(Color32::LIGHT_RED, plugin.errors.to_string())
                        .on_hover_text(text);
                }
                ui.end_row();
            }
        });
    if let Some(plugin) = plugins.iter().find(|plugin| plugin.errors > 0) {
        ui.label(
            RichText::new(format!(
                "{}: {}",
                plugin.name,
                plugin.last_error.as_deref().unwrap_or("last error")
            ))
            .small()
            .color(Color32::LIGHT_RED),
        );
    }
}

/// Compact one-line summary for the app's status bar.
pub fn summary_line(world: &DebugWorld<'_>) -> String {
    let audio = world.audio.load_percent();
    #[cfg(feature = "system")]
    if let Some(stats) = world.system.map(SystemMonitor::stats) {
        return format!(
            "cpu {:.0} % · ram {} · audio {audio:.0} %",
            stats.cpu_percent,
            format_bytes(stats.process_ram)
        );
    }
    format!("audio {audio:.0} %")
}
