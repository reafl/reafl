//! VST3 hosting through the `vst3-host` crate.
//!
//! `vst3_host::Plugin` is `Send` but not `Sync`, and its own playback helpers
//! share it as `Arc<Mutex<Plugin>>`; this module does the same so the UI can
//! attach the editor while the audio thread renders blocks.

use crate::{BundleInfo, HostError, HostResult, ParameterInfo, ParentWindow};
use std::sync::{Arc, Mutex};

use vst3_host::{WindowHandle, midi::MidiChannel, plugin::Plugin};

/// Wraps the parent window in the handle type `vst3-host` expects.
///
/// Every platform has its own constructor because VST3 types the embedding
/// handle differently: an `HWND` on Windows, an `NSView` on macOS and a 32-bit
/// X11 window id on Linux.
///
/// # Parameters
///
/// - `parent`: the native window the editor attaches to.
///
/// # Returns
///
/// The platform handle, or an error on a platform without an embedding path.
///
/// # Errors
///
/// Returns [`HostError::NoEditor`] when the target has no embedding backend.
fn window_handle(parent: ParentWindow) -> HostResult<WindowHandle> {
    #[cfg(windows)]
    {
        // SAFETY: the caller keeps the parent window alive while the editor is
        // attached, which is all `from_hwnd` requires.
        return Ok(unsafe { WindowHandle::from_hwnd(parent.0 as *mut core::ffi::c_void) });
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: as above, the `NSView` outlives the attachment.
        return Ok(unsafe { WindowHandle::from_nsview(parent.0 as *mut core::ffi::c_void) });
    }
    #[cfg(target_os = "linux")]
    {
        // The VST3 X11 platform type expects the window id itself, not a
        // pointer to it.
        return Ok(WindowHandle::from_x11(parent.0 as u32));
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = parent;
        Err(HostError::NoEditor(
            "plug-in editors cannot be embedded on this platform".to_owned(),
        ))
    }
}

struct Core {
    plugin: Mutex<Plugin>,
    info: BundleInfo,
}

pub struct Vst3Ui {
    core: Arc<Core>,
}

pub struct Vst3Audio {
    core: Arc<Core>,
    /// Silent bus buffers reused for every callback, allocated once.
    buffers: vst3_host::audio::BusAudioBuffers,
    block: usize,
    instrument: bool,
}

fn bundle_info(plugin: &Plugin) -> BundleInfo {
    let info = plugin.info();
    BundleInfo {
        name: info.name.clone(),
        vendor: info.vendor.clone(),
        version: info.version.clone(),
        supports_editor: plugin.has_editor(),
        audio_outputs: info.audio_outputs as usize,
        accepts_midi: info.has_midi_input,
    }
}

pub fn load(path: &str, instrument: bool) -> HostResult<(Vst3Ui, Vst3Audio)> {
    let plugin =
        vst3_host::simple::load_plugin(path).map_err(|e| HostError::Vst3(e.to_string()))?;
    let info = bundle_info(&plugin);
    let block = 64;
    let buffers = plugin
        .create_bus_audio_buffers(block)
        .map_err(|e| HostError::Vst3(e.to_string()))?;
    let core = Arc::new(Core {
        plugin: Mutex::new(plugin),
        info,
    });
    Ok((
        Vst3Ui {
            core: Arc::clone(&core),
        },
        Vst3Audio {
            core,
            buffers,
            block,
            instrument,
        },
    ))
}

impl Vst3Ui {
    pub fn info(&self) -> &BundleInfo {
        &self.core.info
    }

    pub fn parameters(&self) -> Vec<ParameterInfo> {
        let Ok(plugin) = self.core.plugin.lock() else {
            return Vec::new();
        };
        plugin
            .get_parameters()
            .map(|list| {
                list.into_iter()
                    .map(|parameter| ParameterInfo {
                        id: parameter.id,
                        name: parameter.name,
                        value: parameter.value as f32,
                        default: parameter.default as f32,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_parameter(&mut self, id: u32, value: f32) {
        if let Ok(mut plugin) = self.core.plugin.lock() {
            let _ = plugin.set_parameter(id, f64::from(value));
        }
    }

    pub fn editor_size(&mut self) -> Option<(u32, u32)> {
        let plugin = self.core.plugin.lock().ok()?;
        let (width, height) = plugin.get_editor_size().ok()?;
        Some((width.max(1) as u32, height.max(1) as u32))
    }

    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        let plugin = self.core.plugin.lock().ok()?;
        plugin.save_state().ok()
    }

    pub fn load_state(&mut self, data: &[u8]) -> bool {
        let Ok(mut plugin) = self.core.plugin.lock() else {
            return false;
        };
        plugin.load_state(data).is_ok()
    }

    pub fn save_preset(&mut self, path: &str) -> HostResult<()> {
        let plugin = self
            .core
            .plugin
            .lock()
            .map_err(|_| HostError::Vst3("plug-in lock poisoned".to_owned()))?;
        plugin
            .save_vstpreset(path)
            .map_err(|error| HostError::Vst3(error.to_string()))
    }

    /// Last parameter the plug-in's own editor changed since the last call.
    pub fn take_last_tweaked(&mut self) -> Option<(u32, f32)> {
        let Ok(mut plugin) = self.core.plugin.lock() else {
            return None;
        };
        let edits = plugin.take_parameter_edits();
        edits
            .iter()
            .rev()
            .find_map(|edit| edit.value.map(|value| (edit.id, value as f32)))
    }

    /// Resizes the attached editor; returns the size the plug-in accepted.
    pub fn resize_editor(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        let mut plugin = self.core.plugin.lock().ok()?;
        let (width, height) = plugin.resize_editor(width as i32, height as i32).ok()?;
        Some((width.max(1) as u32, height.max(1) as u32))
    }

    pub fn open_editor(
        &mut self,
        parent: ParentWindow,
        width: u32,
        height: u32,
    ) -> HostResult<(u32, u32)> {
        let Ok(mut plugin) = self.core.plugin.lock() else {
            return Err(HostError::Vst3("plug-in lock poisoned".to_owned()));
        };
        if !plugin.has_editor() {
            return Err(HostError::NoEditor(format!(
                "{} has no editor",
                self.core.info.name
            )));
        }
        let _ = plugin.resize_editor(width as i32, height as i32);
        let handle = window_handle(parent)?;
        plugin
            .open_editor(handle)
            .map_err(|e| HostError::Vst3(e.to_string()))?;
        let (actual_width, actual_height) = plugin
            .attached_editor_size()
            .ok()
            .flatten()
            .unwrap_or((width as i32, height as i32));
        Ok((actual_width.max(1) as u32, actual_height.max(1) as u32))
    }

    pub fn close_editor(&mut self) {
        if let Ok(mut plugin) = self.core.plugin.lock() {
            let _ = plugin.close_editor();
        }
    }
}

impl Vst3Audio {
    pub fn activate(&mut self, sample_rate: f64, block: usize) {
        if block != self.block {
            return;
        }
        let Ok(mut plugin) = self.core.plugin.lock() else {
            return;
        };
        let _ = plugin.reconfigure(sample_rate, block);
        if let Ok(buffers) = plugin.create_bus_audio_buffers(block) {
            self.buffers = buffers;
        }
        let _ = plugin.start_processing();
    }

    pub fn set_parameter(&mut self, id: u32, value: f32) {
        if let Ok(mut plugin) = self.core.plugin.lock() {
            let _ = plugin.set_parameter(id, f64::from(value));
        }
    }

    pub fn send_note(&mut self, note: u8, velocity: f32, on: bool) {
        let Ok(mut plugin) = self.core.plugin.lock() else {
            return;
        };
        if on {
            let level = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
            let _ = plugin.send_midi_note(note, level.max(1), MidiChannel::Ch1);
        } else {
            let _ = plugin.send_midi_note_off(note, MidiChannel::Ch1);
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len()).min(self.block);
        if frames == 0 {
            return;
        }
        let Ok(mut plugin) = self.core.plugin.lock() else {
            return;
        };
        for bus in &mut self.buffers.outputs {
            for channel in &mut bus.channels {
                channel.fill(0.0);
            }
        }
        if !self.instrument {
            let channels: Vec<&mut Vec<f32>> = self
                .buffers
                .inputs
                .first_mut()
                .map(|bus| bus.channels.iter_mut().collect())
                .unwrap_or_default();
            for (index, channel) in channels.into_iter().enumerate() {
                if index == 0 {
                    channel[..frames].copy_from_slice(&left[..frames]);
                } else if index == 1 {
                    channel[..frames].copy_from_slice(&right[..frames]);
                } else {
                    channel[..frames].fill(0.0);
                }
            }
        } else {
            for bus in &mut self.buffers.inputs {
                for channel in &mut bus.channels {
                    channel[..frames].fill(0.0);
                }
            }
        }
        if plugin.process_bus_audio(&mut self.buffers).is_err() {
            return;
        }
        let Some(bus) = self.buffers.outputs.first() else {
            return;
        };
        if let Some(channel) = bus.channels.first() {
            left[..frames].copy_from_slice(&channel[..frames]);
        }
        if let Some(channel) = bus.channels.get(1) {
            right[..frames].copy_from_slice(&channel[..frames]);
        } else if let Some(channel) = bus.channels.first() {
            right[..frames].copy_from_slice(&channel[..frames]);
        }
    }

    pub fn reset(&mut self) {
        if let Ok(mut plugin) = self.core.plugin.lock() {
            let _ = plugin.midi_panic();
        }
    }
}
