//! GPU reduction for the debug graphs.
//!
//! The overview strips (frame time, audio DSP load) need min/max/mean for one
//! pixel column out of thousands of samples. That is a textbook parallel
//! reduction, so it runs on a compute shader: one invocation per column, each
//! looping over its slice. The result is read back into a small buffer and
//! drawn by egui.
//!
//! The GL context only exists inside a paint callback, so [`GpuHistogram::paint`]
//! uploads the samples, dispatches, and stores the read-back columns for the
//! next frame. When the driver lacks GL 4.3 / `GL_ARB_compute_shader`, or any
//! step fails, [`GpuHistogram::available`] turns false and the caller falls
//! back to [`columns`], the identical CPU reduction.

use std::sync::{Arc, Mutex};

use egui_glow::CallbackFn;
use glow::HasContext;

/// One computed column.
#[derive(Clone, Copy, Debug, Default)]
pub struct Column {
    pub min: f32,
    pub max: f32,
    pub mean: f32,
}

/// Largest column count the output buffer is sized for.
const MAX_COLUMNS: usize = 4096;

/// One invocation per column; each invocation scans its own slice.
const SHADER: &str = r#"#version 430
layout(local_size_x = 1) in;
layout(std430, binding = 0) readonly buffer Samples { float samples[]; };
layout(std430, binding = 1) writeonly buffer Out { vec4 columns[]; };
uniform int u_count;
uniform int u_columns;
void main() {
    uint column = gl_GlobalInvocationID.x;
    if (column >= uint(u_columns)) { return; }
    int begin = int(uint(u_count) * column / uint(u_columns));
    int end = int(uint(u_count) * (column + 1u) / uint(u_columns));
    float low = 3.4e38;
    float high = -3.4e38;
    float sum = 0.0;
    int count = 0;
    for (int index = begin; index < end; index++) {
        float value = samples[index];
        low = min(low, value);
        high = max(high, value);
        sum += value;
        count++;
    }
    columns[column] = count > 0
        ? vec4(low, high, sum / float(count), float(count))
        : vec4(0.0);
}
"#;

/// CPU reduction with the same semantics as the shader.
pub fn columns(samples: &[f32], column_count: usize) -> Vec<Column> {
    let column_count = column_count.max(1);
    (0..column_count)
        .map(|column| {
            let begin = samples.len() * column / column_count;
            let end = samples.len() * (column + 1) / column_count;
            let slice = &samples[begin..end.max(begin)];
            if slice.is_empty() {
                return Column::default();
            }
            let mut min = f32::MAX;
            let mut max = f32::MIN;
            let mut sum = 0.0;
            for &value in slice {
                min = min.min(value);
                max = max.max(value);
                sum += value;
            }
            Column {
                min,
                max,
                mean: sum / slice.len() as f32,
            }
        })
        .collect()
}

#[derive(Default)]
struct Resources {
    program: Option<glow::Program>,
    samples: Option<glow::Buffer>,
    output: Option<glow::Buffer>,
    samples_bytes: usize,
    result: Vec<Column>,
    supported: Option<bool>,
    error: Option<String>,
    /// Column count and sample count of the last dispatch.
    dispatched: (usize, usize),
}

/// Compute-shader accelerated column reduction with a CPU fallback.
pub struct GpuHistogram {
    state: Arc<Mutex<Resources>>,
}

impl Default for GpuHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuHistogram {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Resources::default())),
        }
    }

    /// Whether the last attempt to use the GPU succeeded.
    pub fn available(&self) -> bool {
        match self.state.lock() {
            Ok(state) => state.supported.unwrap_or(false) && state.error.is_none(),
            Err(_) => false,
        }
    }

    /// Last error, for the panel's status line.
    pub fn error(&self) -> Option<String> {
        self.state.lock().ok().and_then(|state| state.error.clone())
    }

    /// Columns computed by the previous frame's dispatch.
    pub fn result(&self) -> Vec<Column> {
        self.state
            .lock()
            .map(|state| state.result.clone())
            .unwrap_or_default()
    }

    /// Dispatches the reduction for this frame. Call once per frame with the
    /// rect that holds the graph; egui runs the callback during painting, and
    /// the read-back columns are visible on the next frame.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        samples: &[f32],
        columns: usize,
    ) {
        if samples.len() < 2 || columns == 0 {
            return;
        }
        let samples: Vec<f32> = samples.to_vec();
        let columns = columns.min(MAX_COLUMNS);
        let state = Arc::clone(&self.state);
        let callback = CallbackFn::new(move |_info, gl_painter| {
            let gl = gl_painter.gl();
            let Ok(mut state) = state.lock() else {
                return;
            };
            if let Err(error) = dispatch(gl, &mut state, &samples, columns) {
                state.error = Some(error);
                state.supported = Some(false);
            }
        });
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
    }
}

/// Runs the compute dispatch and read-back. All GL calls happen on the paint
/// thread, which is the only thread allowed to touch the context.
fn dispatch(
    gl: &glow::Context,
    state: &mut Resources,
    samples: &[f32],
    columns: usize,
) -> Result<(), String> {
    // SAFETY: every call below runs on the paint thread with egui's current
    // context; the buffers and program are created and destroyed there.
    unsafe {
        if state.supported.is_none() {
            let version = gl.version();
            let extensions = gl.supported_extensions();
            let usable = version.major > 4
                || (version.major == 4 && version.minor >= 3)
                || extensions.contains("GL_ARB_compute_shader");
            state.supported = Some(usable);
            if !usable {
                return Err(format!(
                    "GL {}.{} without compute shader support",
                    version.major, version.minor
                ));
            }
        }
        if state.supported != Some(true) {
            return Err("compute shaders are unavailable".to_owned());
        }

        if state.program.is_none() {
            let program = gl.create_program()?;
            let shader = gl.create_shader(glow::COMPUTE_SHADER)?;
            gl.shader_source(shader, SHADER);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                gl.delete_program(program);
                return Err(format!("compute shader failed to compile: {log}"));
            }
            gl.attach_shader(program, shader);
            gl.link_program(program);
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(format!("compute program failed to link: {log}"));
            }
            state.samples = Some(gl.create_buffer()?);
            state.output = Some(gl.create_buffer()?);
            state.program = Some(program);
            // Column output never changes size.
            gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, state.output);
            gl.buffer_data_size(
                glow::SHADER_STORAGE_BUFFER,
                (MAX_COLUMNS * 4 * 4) as i32,
                glow::DYNAMIC_DRAW,
            );
        }

        let sample_count = samples.len();
        let sample_bytes = sample_count * 4;
        let sample_slice = std::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), sample_bytes);
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, state.samples);
        if state.samples_bytes < sample_bytes {
            gl.buffer_data_u8_slice(glow::SHADER_STORAGE_BUFFER, sample_slice, glow::STREAM_DRAW);
            state.samples_bytes = sample_bytes;
        } else {
            gl.buffer_sub_data_u8_slice(glow::SHADER_STORAGE_BUFFER, 0, sample_slice);
        }

        let program = state.program.ok_or("compute program missing")?;
        gl.use_program(Some(program));
        gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, state.samples);
        gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, state.output);
        if let Some(location) = gl.get_uniform_location(program, "u_count") {
            gl.uniform_1_i32(Some(&location), sample_count as i32);
        }
        if let Some(location) = gl.get_uniform_location(program, "u_columns") {
            gl.uniform_1_i32(Some(&location), columns as i32);
        }
        gl.dispatch_compute(columns as u32, 1, 1);
        gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);

        let read_bytes = columns * 4 * 4;
        let mut readback = vec![0u8; read_bytes];
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, state.output);
        gl.get_buffer_sub_data(glow::SHADER_STORAGE_BUFFER, 0, &mut readback);
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, None);
        gl.use_program(None);

        state.result = readback
            .chunks_exact(16)
            .map(|chunk| Column {
                min: f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                max: f32::from_ne_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                mean: f32::from_ne_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]),
            })
            .collect();
        state.dispatched = (columns, sample_count);
        state.error = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_columns_match_slices() {
        let samples: Vec<f32> = (0..100).map(|value| value as f32).collect();
        let columns = columns(&samples, 10);
        assert_eq!(columns.len(), 10);
        assert_eq!(columns[0].min, 0.0);
        assert_eq!(columns[0].max, 9.0);
        assert_eq!(columns[0].mean, 4.5);
        assert_eq!(columns[9].min, 90.0);
        assert_eq!(columns[9].max, 99.0);
    }

    #[test]
    fn empty_input_yields_zeroed_columns() {
        let columns = columns(&[], 4);
        assert_eq!(columns.len(), 4);
        assert!(columns.iter().all(|column| column.max == 0.0));
    }

    #[test]
    fn gpu_starts_unavailable() {
        let histogram = GpuHistogram::new();
        assert!(!histogram.available());
        assert!(histogram.result().is_empty());
    }
}
