param([string]$Root = "crates/reafl-core/src/audio")

$common = @(
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
    '};'
)

$modules = @{
    'engine.rs' = @('use super::model::*;', 'use super::dsp::*;')
    'dsp.rs' = @('use super::model::*;', 'use super::engine::*;')
    'tests.rs' = @('use super::model::*;', 'use super::engine::*;', 'use super::dsp::*;')
}

foreach ($entry in $modules.GetEnumerator()) {
    $path = Join-Path $Root $entry.Key
    $lines = Get-Content -LiteralPath $path
    $doc = @()
    foreach ($line in $lines) { if ($line -match '^//!') { $doc += $line } }
    $body = $lines | Where-Object { $_ -notmatch '^//!' -and $_ -notmatch '^use super::' -and $_ -ne '' }
    $body = $body | ForEach-Object {
        if ($_ -match '^(fn|struct|enum|const) ') { $_ -replace '^(fn|struct|enum|const) ', 'pub(crate) $1 ' }
        else { $_ }
    }
    Set-Content -LiteralPath $path -Value ($doc + @('') + $common + $entry.Value + @('') + $body)
    Write-Host "reheaded $($entry.Key)"
}
