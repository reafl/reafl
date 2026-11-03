//! CLAP hosting through the `clack-host` crate.
//!
//! CLAP splits a plug-in across threads by design: `PluginInstance` is `!Send`
//! and must stay on the thread that created it, while
//! `StartedPluginAudioProcessor` is `Send` and is meant to be driven from the
//! audio thread. [`ClapUi`] keeps the former, [`ClapAudio`] owns the latter.

use std::sync::{Arc, Mutex};

use crate::{BundleInfo, HostError, HostResult, ParameterInfo, ParentWindow};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGuiImpl, PluginGui, Window,
};
use clack_extensions::params::{ParamInfoBuffer, PluginParams};
use clack_host::events::event_types::{NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_host::events::io::{EventBuffer, InputEvents, OutputEvents};
use clack_host::host::HostError as ClapHostError;
use clack_host::prelude::*;
use clack_host::process::audio_buffers::{
    AudioPortBuffer, AudioPortBufferType, AudioPorts, InputChannel,
};
use clack_host::utils::ClapId;

/// Reported to plug-ins in their host window; also shown in "about" boxes.
const HOST_NAME: &str = "PulseForge";
const HOST_VENDOR: &str = "PulseForge";
const HOST_URL: &str = "https://example.invalid/pulseforge";
const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parameter writes from the UI thread are handed to the audio thread through
/// this queue, because CLAP only applies values reliably as process events.
type ParameterQueue = Arc<Mutex<Vec<(u32, f32)>>>;
/// Last parameter the plug-in's own editor changed; written by the audio thread
/// from the process output events and read by the UI thread.
type LastTweaked = Arc<Mutex<Option<u32>>>;

struct SharedHandlers;

impl SharedHandler<'_> for SharedHandlers {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

impl HostGuiImpl for SharedHandlers {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, _size: GuiSize) -> Result<(), ClapHostError> {
        // The wrapper window owns its size; resize hints are accepted and then
        // ignored so a chatty plug-in cannot fight the DAW's layout.
        Ok(())
    }

    fn request_show(&self) -> Result<(), ClapHostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), ClapHostError> {
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {}
}

struct Host;

impl HostHandlers for Host {
    type Shared<'a> = SharedHandlers;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<clack_extensions::gui::HostGui>();
    }
}

pub struct ClapUi {
    instance: PluginInstance<Host>,
    info: BundleInfo,
    parameters: Vec<ParameterInfo>,
    parameters_queue: ParameterQueue,
    last_tweaked: LastTweaked,
    editor_open: bool,
}

pub struct ClapAudio {
    processor: StartedPluginAudioProcessor<Host>,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    events: EventBuffer,
    output_events: EventBuffer,
    scratch_left: Vec<f32>,
    scratch_right: Vec<f32>,
    pending_notes: Vec<(u8, f32, bool)>,
    pending_parameters: ParameterQueue,
    last_tweaked: LastTweaked,
    instrument: bool,
}

fn host_info() -> HostResult<HostInfo> {
    HostInfo::new(HOST_NAME, HOST_VENDOR, HOST_URL, HOST_VERSION)
        .map_err(|e| HostError::Clap(e.to_string()))
}

/// Picks the first descriptor matching the requested role.
fn descriptor_index(factory: &clack_host::factory::plugin::PluginFactory<'_>, want: bool) -> u32 {
    for index in 0..factory.plugin_count() {
        let Some(descriptor) = factory.plugin_descriptor(index) else {
            continue;
        };
        let is_instrument = descriptor
            .features()
            .any(|feature| feature.to_str().map(|f| f == "instrument").unwrap_or(false));
        if is_instrument == want {
            return index;
        }
    }
    0
}

pub fn load(
    path: &str,
    instrument: bool,
    sample_rate: f64,
    block: usize,
) -> HostResult<(ClapUi, ClapAudio)> {
    // SAFETY: the user picked this bundle from a plug-in folder they configured.
    let entry = unsafe { PluginEntry::load(path) }.map_err(|e| HostError::Clap(e.to_string()))?;
    let factory = entry
        .get_plugin_factory()
        .ok_or_else(|| HostError::Clap("bundle exposes no plug-in factory".to_owned()))?;
    let descriptor = factory
        .plugin_descriptor(descriptor_index(&factory, instrument))
        .ok_or_else(|| HostError::Clap("bundle contains no plug-ins".to_owned()))?;
    let id = descriptor
        .id()
        .ok_or_else(|| HostError::Clap("descriptor has no id".to_owned()))?;
    let name = descriptor
        .name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unnamed".to_owned());
    let vendor = descriptor
        .vendor()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let version = descriptor
        .version()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();

    let queue: ParameterQueue = Arc::new(Mutex::new(Vec::new()));
    let last_tweaked: LastTweaked = Arc::new(Mutex::new(None));
    let info = host_info()?;
    let mut instance = PluginInstance::<Host>::new(|_| SharedHandlers, |_| (), &entry, id, &info)
        .map_err(|e| HostError::Clap(e.to_string()))?;

    let (supports_editor, parameters) = {
        let handle = instance.plugin_handle();
        let has_gui = handle.get_extension::<PluginGui>().is_some();
        let parameters = match handle.get_extension::<PluginParams>() {
            Some(params) => read_parameters(&params, &handle),
            None => Vec::new(),
        };
        (has_gui, parameters)
    };

    let block = block.max(1);
    let stopped = instance
        .activate(
            |_, _| (),
            PluginAudioConfiguration {
                sample_rate,
                min_frames_count: 1,
                max_frames_count: block as u32,
            },
        )
        .map_err(|e| HostError::Clap(e.to_string()))?;
    let processor = stopped
        .start_processing()
        .map_err(|e| HostError::Clap(e.to_string()))?;

    let ui = ClapUi {
        instance,
        info: BundleInfo {
            name,
            vendor,
            version,
            supports_editor,
            audio_outputs: 2,
            accepts_midi: instrument,
        },
        parameters,
        parameters_queue: queue,
        last_tweaked: Arc::clone(&last_tweaked),
        editor_open: false,
    };
    let audio = ClapAudio {
        processor,
        input_ports: AudioPorts::with_capacity(2, 1),
        output_ports: AudioPorts::with_capacity(2, 1),
        events: EventBuffer::with_capacity(32),
        output_events: EventBuffer::with_capacity(32),
        scratch_left: vec![0.0; block],
        scratch_right: vec![0.0; block],
        pending_notes: Vec::with_capacity(32),
        pending_parameters: Arc::new(Mutex::new(Vec::new())),
        last_tweaked,
        instrument,
    };
    Ok((ui, audio))
}

fn read_parameters(
    params: &PluginParams,
    handle: &PluginMainThreadHandle<'_>,
) -> Vec<ParameterInfo> {
    let mut out = Vec::new();
    let mut buffer = ParamInfoBuffer::new();
    for index in 0..params.count(handle) {
        let Some(info) = params.get_info(handle, index, &mut buffer) else {
            continue;
        };
        let value = params
            .get_value(handle, info.id)
            .map(|v| normalise(v, info.min_value, info.max_value))
            .unwrap_or(0.0);
        out.push(ParameterInfo {
            id: info.id.get(),
            name: String::from_utf8_lossy(info.name).into_owned(),
            value,
            default: normalise(info.default_value, info.min_value, info.max_value),
        });
    }
    out
}

fn normalise(value: f64, min: f64, max: f64) -> f32 {
    if (max - min).abs() < f64::EPSILON {
        0.0
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
    }
}

impl ClapUi {
    pub fn info(&self) -> &BundleInfo {
        &self.info
    }

    pub fn parameters(&self) -> Vec<ParameterInfo> {
        self.parameters.clone()
    }

    /// Queues a normalized value for the audio thread. CLAP only guarantees a
    /// value reaches the plug-in through a `param_value` event inside a block,
    /// so the write is forwarded rather than applied here.
    pub fn set_parameter(&mut self, id: u32, value: f32) {
        if let Ok(mut queue) = self.parameters_queue.lock() {
            queue.push((id, value));
        }
    }

    /// CLAP only allows `get_size` after `create`, so this reports a size only
    /// while an editor is open.
    pub fn editor_size(&mut self) -> Option<(u32, u32)> {
        if !self.editor_open {
            return None;
        }
        let handle = self.instance.plugin_handle();
        let gui = handle.get_extension::<PluginGui>()?;
        let size = gui.get_size(&handle)?;
        Some((size.width.max(1), size.height.max(1)))
    }

    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        use clack_extensions::state::PluginState;
        let handle = self.instance.plugin_handle();
        let state = handle.get_extension::<PluginState>()?;
        let mut data = Vec::new();
        match state.save(&handle, &mut data) {
            Ok(()) if !data.is_empty() => Some(data),
            _ => None,
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> bool {
        use clack_extensions::state::PluginState;
        let handle = self.instance.plugin_handle();
        let Some(state) = handle.get_extension::<PluginState>() else {
            return false;
        };
        state.load(&handle, &mut &data[..]).is_ok()
    }

    /// CLAP has no preset container, so the raw state is written as-is.
    pub fn save_preset(&mut self, path: &str) -> HostResult<()> {
        let data = self
            .save_state()
            .ok_or_else(|| HostError::Clap("plug-in exposes no state".to_owned()))?;
        std::fs::write(path, data).map_err(|error| HostError::Clap(error.to_string()))
    }

    /// Parameter the plug-in's editor most recently changed, if any.
    pub fn take_last_tweaked(&mut self) -> Option<(u32, f32)> {
        let id = self
            .last_tweaked
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())?;
        let value = self
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .map_or(0.5, |parameter| parameter.value);
        Some((id, value))
    }

    /// Resizes the editor; CLAP refuses for non-resizable plug-ins, in which
    /// case the previous size is returned.
    pub fn resize_editor(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if !self.editor_open {
            return None;
        }
        let handle = self.instance.plugin_handle();
        let gui = handle.get_extension::<PluginGui>()?;
        let requested = GuiSize { width, height };
        let size = gui.adjust_size(&handle, requested).unwrap_or(requested);
        if gui.set_size(&handle, size).is_err() {
            return gui.get_size(&handle).map(|size| (size.width, size.height));
        }
        Some((size.width.max(1), size.height.max(1)))
    }

    pub fn open_editor(
        &mut self,
        parent: ParentWindow,
        width: u32,
        height: u32,
    ) -> HostResult<(u32, u32)> {
        let handle = self.instance.plugin_handle();
        let Some(gui) = handle.get_extension::<PluginGui>() else {
            return Err(HostError::NoEditor(format!(
                "{} has no editor",
                self.info.name
            )));
        };
        let api_type = GuiApiType::default_for_current_platform()
            .ok_or_else(|| HostError::Clap("no GUI API on this platform".to_owned()))?;
        let configuration = GuiConfiguration {
            api_type,
            is_floating: false,
        };
        if !gui.is_api_supported(&handle, configuration) {
            return Err(HostError::NoEditor(format!(
                "{} does not support an embedded editor",
                self.info.name
            )));
        }
        gui.create(&handle, configuration)
            .map_err(|e| HostError::Clap(e.to_string()))?;
        let preferred = gui.get_size(&handle).unwrap_or(GuiSize { width, height });
        // SAFETY: the caller keeps `parent` alive while the editor is open.
        let window = Window::from_generic_ptr(api_type, parent.0 as *mut _);
        if let Err(error) = unsafe { gui.set_parent(&handle, window) } {
            gui.destroy(&handle);
            return Err(HostError::Clap(error.to_string()));
        }
        gui.show(&handle)
            .map_err(|e| HostError::Clap(e.to_string()))?;
        self.editor_open = true;
        Ok((preferred.width.max(1), preferred.height.max(1)))
    }

    pub fn close_editor(&mut self) {
        if !self.editor_open {
            return;
        }
        let handle = self.instance.plugin_handle();
        if let Some(gui) = handle.get_extension::<PluginGui>() {
            let _ = gui.hide(&handle);
            gui.destroy(&handle);
        }
        self.editor_open = false;
    }
}

impl Drop for ClapUi {
    fn drop(&mut self) {
        self.close_editor();
    }
}

impl ClapAudio {
    /// CLAP activation happens at load time (it needs the non-`Send` instance),
    /// so there is nothing left to do but keep the scratch length in sync.
    pub fn activate(&mut self, _sample_rate: f64, block: usize) {
        self.scratch_left.resize(block.max(1), 0.0);
        self.scratch_right.resize(block.max(1), 0.0);
    }

    pub fn set_parameter(&mut self, id: u32, value: f32) {
        if let Ok(mut queue) = self.pending_parameters.lock() {
            queue.push((id, value));
        }
    }

    pub fn send_note(&mut self, note: u8, velocity: f32, on: bool) {
        self.pending_notes.push((note, velocity, on));
    }

    pub fn reset(&mut self) {
        self.pending_notes.clear();
        self.processor.reset();
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len()).min(self.scratch_left.len());
        if frames == 0 {
            return;
        }
        self.events.clear();
        if let Ok(mut queue) = self.pending_parameters.try_lock() {
            for (id, value) in queue.drain(..) {
                if let Some(clap_id) = ClapId::from_raw(id) {
                    self.events.push(&ParamValueEvent::new(
                        0,
                        clap_id,
                        Pckn::match_all(),
                        f64::from(value),
                    ));
                }
            }
        }
        for (note, velocity, on) in self.pending_notes.drain(..) {
            let pckn = Pckn::new(0u16, 0u16, u16::from(note), clack_host::events::Match::All);
            if on {
                self.events.push(&NoteOnEvent::new(
                    0,
                    pckn,
                    f64::from(velocity).clamp(0.0, 1.0),
                ));
            } else {
                self.events.push(&NoteOffEvent::new(0, pckn, 0.0));
            }
        }
        let input_events = InputEvents::from_buffer(&self.events);
        let mut output_events = OutputEvents::from_buffer(&mut self.output_events);

        if self.instrument {
            let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    [left[..frames].as_mut(), right[..frames].as_mut()].into_iter(),
                ),
            }]);
            let _ = self.processor.process(
                &InputAudioBuffers::empty(),
                &mut output_audio,
                &input_events,
                &mut output_events,
                None,
                None,
            );
        } else {
            self.scratch_left[..frames].copy_from_slice(&left[..frames]);
            self.scratch_right[..frames].copy_from_slice(&right[..frames]);
            let scratch_left = &mut self.scratch_left[..frames];
            let scratch_right = &mut self.scratch_right[..frames];
            let input_audio = self.input_ports.with_input_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    [scratch_left, scratch_right]
                        .into_iter()
                        .map(InputChannel::variable),
                ),
            }]);
            let mut output_audio = self.output_ports.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    [left[..frames].as_mut(), right[..frames].as_mut()].into_iter(),
                ),
            }]);
            let _ = self.processor.process(
                &input_audio,
                &mut output_audio,
                &input_events,
                &mut output_events,
                None,
                None,
            );
        }
        self.record_parameter_changes();
    }

    /// Remembers the last `param_value` the plug-in emitted, which is how a
    /// CLAP editor reports that the user turned a knob.
    fn record_parameter_changes(&mut self) {
        let mut last = None;
        for index in 0..self.output_events.len() {
            let Some(event) = self.output_events.get(index) else {
                continue;
            };
            if let Some(change) = event.as_event::<ParamValueEvent>()
                && let Some(id) = change.param_id()
            {
                last = Some(id.get());
            }
        }
        if let Some(id) = last
            && let Ok(mut slot) = self.last_tweaked.lock()
        {
            *slot = Some(id);
        }
        self.output_events.clear();
    }
}
