param([string]$Root = "crates/reafl-core/src/audio")

# The submodules need the same imports the monolith had, and the data types they
# share have to be visible across the module tree.
$header = @(
    'use std::{',
    '    collections::HashMap,',
    '    f32::consts::TAU,',
    '    sync::{',
    '        Arc, Mutex,',
    '        atomic::{AtomicU32, AtomicU64, Ordering},',
    '        mpsc::{self, SyncSender},',
    '    },',
    '    time::{Duration, Instant},',
    '};',
    '',
    'use cpal::{',
    '    BufferSize, SampleFormat, Stream,',
    '    traits::{DeviceTrait, HostTrait, StreamTrait},',
    '};',
    'use reafl_profiling::{Channel, SlotCounters, profile_scope, set_thread_channel};',
    '',
    'use crate::{',
    '    hosted::{HostedSlot, effect_host_id, instrument_host_id},',
    '    plugins::{BuiltinEffect, EffectParameters},',
    '    project::{',
    '        AutomationLaneParameter, AutomationSource, ClipSource, DefaultSample, EffectParameter,',
    '        Instrument, InstrumentParameter, LfoShape, MixerParameter, ParameterTarget, PianoNote,',
    '        Project, SignalWaveform, SynthKind, TimeUnit, TimeValue, finite_clamp, normalize_sends,',
    '    },',
    '    smooth::SmoothedValue,',
    '};',
    '',
    'use super::model::*;',
    ''
)

foreach ($name in @('engine.rs', 'dsp.rs', 'tests.rs')) {
    $path = Join-Path $Root $name
    $lines = Get-Content -LiteralPath $path
    $body = $lines | Where-Object { $_ -ne 'use super::*;' -and $_ -ne '' }
    Set-Content -LiteralPath $path -Value ($header + $body)
    Write-Host "reheaded $name"
}

# Private data types the sibling modules use.
$modelPath = Join-Path $Root 'model.rs'
$model = Get-Content -LiteralPath $modelPath
$model = $model | ForEach-Object {
    if ($_ -match '^(struct|enum|const|fn) ') { $_ -replace '^(struct|enum|const|fn) ', 'pub(crate) $1 ' }
    else { $_ }
}
Set-Content -LiteralPath $modelPath -Value $model
Write-Host 'model items widened to pub(crate)'
