// For profiling (see also Cargo.toml)
//use jemallocator::Jemalloc;
//
//#[global_allocator]
//static GLOBAL: Jemalloc = Jemalloc;

extern crate nalgebra as na;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::{self, read_to_string, File};
use std::io::{ErrorKind, Write};
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use std::{env, iter};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use radiance::{
    ArcTextureViewSampler, AudioInputDevice, AutoDJ, Context, InputDeviceKind, InsertionPoint,
    Mir, MusicInfo, NodeId, Props, RenderTarget, RenderTargetId,
};

mod ui;
use ui::{library, modal, modal_shown, mosaic, UiBg};
use ui::{BeatWidget, SpectrumWidget, WaveformWidget};

mod setup;
use setup::load_default_library;

mod winit_output;
use winit_output::WinitOutput;

const AUTOSAVE_INTERVAL_FRAMES: usize = 60 * 10;
const AUTOSAVE_FILENAME: &str = "autosave.json";
const APP_SETTINGS_FILENAME: &str = "settings.json";
const DEFAULT_AUDIO_INPUT_LABEL: &str = "Default input";
const DEFAULT_SYSTEM_SOURCE_LABEL: &str = "System default source";
const PERF_LOG_ENV: &str = "RADIANCE_PERF_LOG";
const PERF_LOG_INTERVAL_ENV: &str = "RADIANCE_PERF_LOG_INTERVAL";
const PERF_LOG_DEFAULT_INTERVAL_FRAMES: usize = 240;
const PREVIEW_RENDER_INTERVAL_WITH_VISIBLE_OUTPUT: usize = 2;
const UI_BG_RENDER_INTERVAL_WITH_VISIBLE_OUTPUT: usize = 3;
const GPU_TIMESTAMP_SLOT_COUNT: usize = 32;
const GPU_TIMESTAMP_MAX_QUERIES_PER_SUBMISSION: u32 = 4;

const LOGO_SIZE: egui::Vec2 = egui::Vec2 { x: 56., y: 56. };

#[derive(Debug, Clone, Default)]
struct FramePerformanceSample {
    total: Duration,
    mir_poll: Duration,
    ctx_update: Duration,
    autosave: Duration,
    preview_paint: Duration,
    ui_bg_paint: Duration,
    ui_cpu: Duration,
    output_update: Duration,
    ui_gpu: Duration,
    ui_present: Duration,
    queue_submits: u32,
    graph_paints: u32,
    surface_passes: u32,
    presented_windows: u32,
    did_vsync: bool,
}

impl FramePerformanceSample {
    fn absorb_output_update(&mut self, output: &OutputUpdateStats) {
        self.output_update = output.total;
        self.queue_submits += output.queue_submits;
        self.graph_paints += output.graph_paints;
        self.surface_passes += output.surface_passes;
        self.presented_windows += output.presented_windows;
    }
}

#[derive(Debug, Clone, Default)]
struct OutputUpdateStats {
    total: Duration,
    queue_submits: u32,
    graph_paints: u32,
    surface_passes: u32,
    presented_windows: u32,
    did_vsync: bool,
}

#[derive(Debug, Default)]
struct PerfReporter {
    report_every_frames: usize,
    samples: Vec<FramePerformanceSample>,
}

impl PerfReporter {
    fn from_env() -> Option<Self> {
        if !env_var_truthy(PERF_LOG_ENV) {
            return None;
        }

        let report_every_frames = perf_log_interval_frames();

        eprintln!(
            "Radiance performance logging enabled; reporting every {} frames",
            report_every_frames
        );

        Some(Self {
            report_every_frames,
            samples: Vec::with_capacity(report_every_frames),
        })
    }

    fn push(&mut self, sample: FramePerformanceSample) {
        self.samples.push(sample);
        if self.samples.len() >= self.report_every_frames {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.samples.is_empty() {
            return;
        }

        let avg_total_ms = avg_duration_ms(&self.samples, |sample| sample.total);
        let p95_total_ms = percentile_duration_ms(&self.samples, |sample| sample.total, 0.95);
        let max_total_ms = max_duration_ms(&self.samples, |sample| sample.total);
        let avg_mir_ms = avg_duration_ms(&self.samples, |sample| sample.mir_poll);
        let avg_ctx_update_ms = avg_duration_ms(&self.samples, |sample| sample.ctx_update);
        let avg_autosave_ms = avg_duration_ms(&self.samples, |sample| sample.autosave);
        let avg_preview_ms = avg_duration_ms(&self.samples, |sample| sample.preview_paint);
        let avg_ui_bg_ms = avg_duration_ms(&self.samples, |sample| sample.ui_bg_paint);
        let avg_ui_cpu_ms = avg_duration_ms(&self.samples, |sample| sample.ui_cpu);
        let avg_output_ms = avg_duration_ms(&self.samples, |sample| sample.output_update);
        let avg_ui_gpu_ms = avg_duration_ms(&self.samples, |sample| sample.ui_gpu);
        let avg_ui_present_ms = avg_duration_ms(&self.samples, |sample| sample.ui_present);
        let avg_queue_submits = avg_count(&self.samples, |sample| sample.queue_submits);
        let avg_graph_paints = avg_count(&self.samples, |sample| sample.graph_paints);
        let avg_surface_passes = avg_count(&self.samples, |sample| sample.surface_passes);
        let avg_presented_windows = avg_count(&self.samples, |sample| sample.presented_windows);
        let vsync_ratio = self
            .samples
            .iter()
            .filter(|sample| sample.did_vsync)
            .count() as f64
            / self.samples.len() as f64;

        eprintln!(
            concat!(
                "perf frames={} avg={:.2}ms p95={:.2}ms max={:.2}ms vsync={:.0}% ",
                "| mir={:.2} update={:.2} autosave={:.2} preview={:.2} ui_bg={:.2} ",
                "ui_cpu={:.2} outputs={:.2} ui_gpu={:.2} ui_present={:.2} ",
                "| submits={:.1} graph_paints={:.1} surface_passes={:.1} windows={:.1}"
            ),
            self.samples.len(),
            avg_total_ms,
            p95_total_ms,
            max_total_ms,
            vsync_ratio * 100.0,
            avg_mir_ms,
            avg_ctx_update_ms,
            avg_autosave_ms,
            avg_preview_ms,
            avg_ui_bg_ms,
            avg_ui_cpu_ms,
            avg_output_ms,
            avg_ui_gpu_ms,
            avg_ui_present_ms,
            avg_queue_submits,
            avg_graph_paints,
            avg_surface_passes,
            avg_presented_windows,
        );

        self.samples.clear();
    }
}

#[derive(Debug, Clone, Copy)]
enum GpuTimingStage {
    Preview,
    UiBg,
    Outputs,
    Ui,
}

#[derive(Debug, Default)]
struct GpuStageStats {
    count: usize,
    total_ms: f64,
    max_ms: f64,
}

impl GpuStageStats {
    fn record(&mut self, duration_ms: f64) {
        self.count += 1;
        self.total_ms += duration_ms;
        self.max_ms = self.max_ms.max(duration_ms);
    }

    fn average_ms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_ms / self.count as f64
        }
    }
}

#[derive(Debug, Default)]
struct GpuTimestampReporter {
    report_every_frames: usize,
    preview_frames: usize,
    preview: GpuStageStats,
    ui_bg: GpuStageStats,
    outputs: GpuStageStats,
    ui: GpuStageStats,
}

impl GpuTimestampReporter {
    fn new(report_every_frames: usize) -> Self {
        Self {
            report_every_frames,
            ..Default::default()
        }
    }

    fn record(&mut self, stage: GpuTimingStage, duration_ms: f64) {
        match stage {
            GpuTimingStage::Preview => self.preview.record(duration_ms),
            GpuTimingStage::UiBg => self.ui_bg.record(duration_ms),
            GpuTimingStage::Outputs => self.outputs.record(duration_ms),
            GpuTimingStage::Ui => self.ui.record(duration_ms),
        }

        if matches!(stage, GpuTimingStage::Preview) {
            self.preview_frames += 1;
            if self.preview_frames >= self.report_every_frames {
                self.flush();
            }
        }
    }

    fn flush(&mut self) {
        if self.preview_frames == 0 {
            return;
        }

        eprintln!(
            concat!(
                "gpu frames={} preview={:.2}ms ui_bg={:.2}ms outputs={:.2}ms ui={:.2}ms ",
                "| max preview={:.2} outputs={:.2} ui={:.2}"
            ),
            self.preview_frames,
            self.preview.average_ms(),
            self.ui_bg.average_ms(),
            self.outputs.average_ms(),
            self.ui.average_ms(),
            self.preview.max_ms,
            self.outputs.max_ms,
            self.ui.max_ms,
        );

        self.preview_frames = 0;
        self.preview = GpuStageStats::default();
        self.ui_bg = GpuStageStats::default();
        self.outputs = GpuStageStats::default();
        self.ui = GpuStageStats::default();
    }
}

#[derive(Debug, Clone, Copy)]
struct GpuTimestampSpan {
    stage: GpuTimingStage,
    start_query: u32,
    end_query: u32,
}

#[derive(Debug)]
struct GpuTimestampSlot {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    pending: bool,
    query_count: u32,
    spans: Vec<GpuTimestampSpan>,
}

#[derive(Debug, Clone, Copy)]
struct GpuTimestampSubmission {
    slot_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct GpuTimestampReadback {
    slot_index: usize,
    success: bool,
}

#[derive(Debug)]
struct GpuTimestampProfiler {
    timestamp_period_ns: f64,
    slots: Vec<GpuTimestampSlot>,
    reporter: GpuTimestampReporter,
    readback_tx: mpsc::Sender<GpuTimestampReadback>,
    readback_rx: mpsc::Receiver<GpuTimestampReadback>,
    warned_slot_exhaustion: bool,
}

impl GpuTimestampProfiler {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, report_every_frames: usize) -> Self {
        let query_buffer_size =
            GPU_TIMESTAMP_MAX_QUERIES_PER_SUBMISSION as u64 * std::mem::size_of::<u64>() as u64;
        let slots = (0..GPU_TIMESTAMP_SLOT_COUNT)
            .map(|slot_index| GpuTimestampSlot {
                query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some(&format!("Radiance GPU timestamp query set {}", slot_index)),
                    ty: wgpu::QueryType::Timestamp,
                    count: GPU_TIMESTAMP_MAX_QUERIES_PER_SUBMISSION,
                }),
                resolve_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Radiance GPU timestamp resolve {}", slot_index)),
                    size: query_buffer_size,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                readback_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Radiance GPU timestamp readback {}", slot_index)),
                    size: query_buffer_size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                pending: false,
                query_count: 0,
                spans: Vec::new(),
            })
            .collect();
        let (readback_tx, readback_rx) = mpsc::channel();

        eprintln!(
            "Radiance GPU timestamp logging enabled; query period {:.3}ns",
            queue.get_timestamp_period()
        );

        Self {
            timestamp_period_ns: queue.get_timestamp_period() as f64,
            slots,
            reporter: GpuTimestampReporter::new(report_every_frames),
            readback_tx,
            readback_rx,
            warned_slot_exhaustion: false,
        }
    }

    fn begin_submission(&mut self, stages: &[GpuTimingStage]) -> Option<GpuTimestampSubmission> {
        let slot_index = match self.slots.iter().position(|slot| !slot.pending) {
            Some(slot_index) => slot_index,
            None => {
                if !self.warned_slot_exhaustion {
                    eprintln!("Radiance GPU timestamp logging dropped samples because all slots were busy");
                    self.warned_slot_exhaustion = true;
                }
                return None;
            }
        };

        let slot = &mut self.slots[slot_index];
        slot.pending = true;
        slot.query_count = 0;
        slot.spans.clear();
        for stage in stages {
            let start_query = slot.query_count;
            let end_query = start_query + 1;
            slot.spans.push(GpuTimestampSpan {
                stage: *stage,
                start_query,
                end_query,
            });
            slot.query_count += 2;
        }

        Some(GpuTimestampSubmission { slot_index })
    }

    fn write_stage_start(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        submission: &GpuTimestampSubmission,
        stage_index: usize,
    ) {
        let slot = &self.slots[submission.slot_index];
        encoder.write_timestamp(&slot.query_set, slot.spans[stage_index].start_query);
    }

    fn write_stage_end(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        submission: &GpuTimestampSubmission,
        stage_index: usize,
    ) {
        let slot = &self.slots[submission.slot_index];
        encoder.write_timestamp(&slot.query_set, slot.spans[stage_index].end_query);
    }

    fn encode_resolve(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        submission: &GpuTimestampSubmission,
    ) {
        let slot = &self.slots[submission.slot_index];
        let byte_len = slot.query_count as u64 * std::mem::size_of::<u64>() as u64;
        encoder.resolve_query_set(&slot.query_set, 0..slot.query_count, &slot.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve_buffer, 0, &slot.readback_buffer, 0, byte_len);
    }

    fn schedule_readback(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        submission: GpuTimestampSubmission,
    ) {
        let slot = &self.slots[submission.slot_index];
        let byte_len = slot.query_count as u64 * std::mem::size_of::<u64>() as u64;
        let sender = self.readback_tx.clone();
        command_buffer.map_buffer_on_submit(
            &slot.readback_buffer,
            wgpu::MapMode::Read,
            0..byte_len,
            move |result| {
                let _ = sender.send(GpuTimestampReadback {
                    slot_index: submission.slot_index,
                    success: result.is_ok(),
                });
            },
        );
    }

    fn poll(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);

        while let Ok(readback) = self.readback_rx.try_recv() {
            let slot = &mut self.slots[readback.slot_index];
            if readback.success {
                let byte_len = slot.query_count as u64 * std::mem::size_of::<u64>() as u64;
                let mapped_range = slot.readback_buffer.slice(..byte_len).get_mapped_range();
                let timestamps: &[u64] = bytemuck::cast_slice(&mapped_range);
                for span in &slot.spans {
                    let start = timestamps[span.start_query as usize];
                    let end = timestamps[span.end_query as usize];
                    if end >= start {
                        let duration_ms =
                            (end - start) as f64 * self.timestamp_period_ns / 1_000_000.0;
                        self.reporter.record(span.stage, duration_ms);
                    }
                }
                drop(mapped_range);
                slot.readback_buffer.unmap();
            }

            slot.pending = false;
            slot.query_count = 0;
            slot.spans.clear();
        }
    }
}

fn perf_log_interval_frames() -> usize {
    env::var(PERF_LOG_INTERVAL_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(PERF_LOG_DEFAULT_INTERVAL_FRAMES)
}

fn env_var_truthy(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn avg_duration_ms<F>(samples: &[FramePerformanceSample], selector: F) -> f64
where
    F: Fn(&FramePerformanceSample) -> Duration,
{
    samples
        .iter()
        .map(|sample| selector(sample).as_secs_f64() * 1000.0)
        .sum::<f64>()
        / samples.len() as f64
}

fn max_duration_ms<F>(samples: &[FramePerformanceSample], selector: F) -> f64
where
    F: Fn(&FramePerformanceSample) -> Duration,
{
    samples
        .iter()
        .map(|sample| selector(sample).as_secs_f64() * 1000.0)
        .fold(0.0, f64::max)
}

fn percentile_duration_ms<F>(samples: &[FramePerformanceSample], selector: F, percentile: f64) -> f64
where
    F: Fn(&FramePerformanceSample) -> Duration,
{
    let mut values: Vec<f64> = samples
        .iter()
        .map(|sample| selector(sample).as_secs_f64() * 1000.0)
        .collect();
    values.sort_by(|left, right| left.total_cmp(right));

    let index = ((values.len().saturating_sub(1)) as f64 * percentile).round() as usize;
    values[index]
}

fn avg_count<F>(samples: &[FramePerformanceSample], selector: F) -> f64
where
    F: Fn(&FramePerformanceSample) -> u32,
{
    samples
        .iter()
        .map(|sample| selector(sample) as f64)
        .sum::<f64>()
        / samples.len() as f64
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default)]
    sync_input_device_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemAudioSource {
    name: String,
    label: String,
    is_monitor: bool,
}

fn autosave(resource_dir: &Path, props: &Props) {
    let inner = || {
        let contents = serde_json::to_string(props).map_err(|e| format!("{:?}", e))?;
        let mut file =
            File::create(resource_dir.join(AUTOSAVE_FILENAME)).map_err(|e| format!("{:?}", e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("{:?}", e))?;
        Ok(())
    };

    inner().unwrap_or_else(|msg: String| println!("Failed to write autosave file: {}", msg));
}

fn read_app_settings(resource_dir: &Path) -> Result<AppSettings, String> {
    let contents = match read_to_string(resource_dir.join(APP_SETTINGS_FILENAME)) {
        Ok(contents) => contents,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(AppSettings::default());
        }
        Err(e) => return Err(format!("{:?}", e)),
    };
    serde_json::from_str(contents.as_str()).map_err(|e| format!("{:?}", e))
}

fn write_app_settings(resource_dir: &Path, settings: &AppSettings) {
    let inner = || {
        let contents = serde_json::to_string(settings).map_err(|e| format!("{:?}", e))?;
        let mut file = File::create(resource_dir.join(APP_SETTINGS_FILENAME))
            .map_err(|e| format!("{:?}", e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("{:?}", e))?;
        Ok(())
    };

    inner().unwrap_or_else(|msg: String| println!("Failed to write app settings: {}", msg));
}

fn selected_sync_input_label(device_name: Option<&str>) -> String {
    device_name.unwrap_or(DEFAULT_AUDIO_INPUT_LABEL).to_owned()
}

fn restore_sync_input_device(
    settings: &AppSettings,
    available_devices: &[AudioInputDevice],
) -> Option<String> {
    settings.sync_input_device_name.as_ref().and_then(|device_name| {
        if available_devices
            .iter()
            .any(|device| device.name == *device_name)
        {
            Some(device_name.clone())
        } else {
            println!(
                "Saved sync input '{}' is unavailable; using the default input instead",
                device_name
            );
            None
        }
    })
}

fn show_sync_input_group(
    ui: &mut egui::Ui,
    selected_device_name: &mut String,
    group_label: &str,
    devices: &[&AudioInputDevice],
) {
    if devices.is_empty() {
        return;
    }

    ui.separator();
    ui.label(egui::RichText::new(group_label).weak());
    for device in devices {
        ui.selectable_value(
            selected_device_name,
            device.name.clone(),
            device.name.as_str(),
        );
    }
}

fn show_system_source_group(
    ui: &mut egui::Ui,
    selected_source_name: &mut String,
    group_label: &str,
    sources: &[&SystemAudioSource],
) {
    if sources.is_empty() {
        return;
    }

    ui.separator();
    ui.label(egui::RichText::new(group_label).weak());
    for source in sources {
        ui.selectable_value(selected_source_name, source.name.clone(), source.label.as_str())
            .on_hover_text(source.name.as_str());
    }
}

#[cfg(target_os = "linux")]
fn run_system_command(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to launch {}: {:?}", program, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} {:?} failed with status {}: {}",
            program,
            args,
            output.status,
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "linux")]
fn label_for_system_source(name: &str) -> String {
    let is_monitor = name.ends_with(".monitor");
    let trimmed_name = name.trim_end_matches(".monitor");
    let parts: Vec<&str> = trimmed_name.split("__").collect();
    if let Some(device_name) = parts.get(parts.len().saturating_sub(2)) {
        let readable = device_name.replace('_', " ");
        if is_monitor {
            format!("{} monitor", readable)
        } else {
            readable
        }
    } else {
        name.to_owned()
    }
}

#[cfg(target_os = "linux")]
fn available_system_audio_sources() -> Vec<SystemAudioSource> {
    let output = match run_system_command("pactl", &["list", "short", "sources"]) {
        Ok(output) => output,
        Err(err) => {
            println!("Failed to enumerate system audio sources: {}", err);
            return Vec::new();
        }
    };

    let mut sources: Vec<_> = output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            columns.next()?;
            let name = columns.next()?.to_owned();
            Some(SystemAudioSource {
                label: label_for_system_source(&name),
                is_monitor: name.ends_with(".monitor"),
                name,
            })
        })
        .collect();
    sources.sort_by(|left, right| {
        right
            .is_monitor
            .cmp(&left.is_monitor)
            .then_with(|| left.label.to_ascii_lowercase().cmp(&right.label.to_ascii_lowercase()))
    });
    sources
}

#[cfg(not(target_os = "linux"))]
fn available_system_audio_sources() -> Vec<SystemAudioSource> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn current_radiance_source_output_id() -> Result<String, String> {
    let output = run_system_command("pactl", &["list", "source-outputs"])?;
    for block in output.split("\n\n") {
        if block.contains("application.name = \"PipeWire ALSA [radiance]\"")
            || block.contains("node.name = \"alsa_capture.radiance\"")
        {
            if let Some(header) = block.lines().next() {
                if let Some(id) = header.trim().strip_prefix("Source Output #") {
                    return Ok(id.trim().to_owned());
                }
            }
        }
    }
    Err("Could not find the active Radiance capture stream".to_owned())
}

#[cfg(target_os = "linux")]
fn default_system_audio_source_name() -> Result<String, String> {
    run_system_command("pactl", &["get-default-source"]).map(|output| output.trim().to_owned())
}

#[cfg(target_os = "linux")]
fn move_radiance_capture_to_system_source(source_name: Option<&str>) -> Result<Option<String>, String> {
    let target_source_name = match source_name {
        Some(name) => name.to_owned(),
        None => default_system_audio_source_name()?,
    };
    let source_output_id = current_radiance_source_output_id()?;
    run_system_command(
        "pactl",
        &["move-source-output", source_output_id.as_str(), target_source_name.as_str()],
    )?;
    Ok(Some(target_source_name))
}

#[cfg(not(target_os = "linux"))]
fn move_radiance_capture_to_system_source(_source_name: Option<&str>) -> Result<Option<String>, String> {
    Err("System source routing is only available on Linux".to_owned())
}

fn selected_system_source_label(
    selected_source_name: &str,
    available_sources: &[SystemAudioSource],
) -> String {
    if selected_source_name == DEFAULT_SYSTEM_SOURCE_LABEL {
        return DEFAULT_SYSTEM_SOURCE_LABEL.to_owned();
    }

    available_sources
        .iter()
        .find(|source| source.name == selected_source_name)
        .map(|source| source.label.clone())
        .unwrap_or_else(|| selected_source_name.to_owned())
}

fn main() {
    env_logger::init();

    // Append build-time RADIANCE_ADDITIONAL_PATH to run-time PATH
    // (this allows a MacOS bundle build to add homebrew directories to PATH
    // so that a homebrew-installed yt-dlp can be found)
    if let Some(additional_path) = option_env!("RADIANCE_ADDITIONAL_PATH") {
        if let Ok(current_path) = env::var("PATH") {
            let separator = if cfg!(windows) { ";" } else { ":" };
            let new_path = format!("{}{}{}", current_path, separator, additional_path);
            unsafe {
                env::set_var("PATH", new_path);
            }
        }
    }

    // Prepare wgpu
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::from_env_or_default(),
        memory_budget_thresholds: Default::default(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap();

    let perf_logging_enabled = env_var_truthy(PERF_LOG_ENV);
    let gpu_timestamps_enabled = perf_logging_enabled
        && adapter
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
    if perf_logging_enabled && !gpu_timestamps_enabled {
        eprintln!(
            "Radiance GPU timestamp logging unavailable; adapter lacks TIMESTAMP_QUERY_INSIDE_ENCODERS"
        );
    }
    let mut required_features = wgpu::Features::TEXTURE_BINDING_ARRAY;
    if gpu_timestamps_enabled {
        required_features |=
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    }

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features,
        // WebGL doesn't support all of wgpu's features, so if
        // we're building for the web we'll have to disable some.
        required_limits: if cfg!(target_arch = "wasm32") {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits {
                max_binding_array_elements_per_shader_stage: 500_000,
                ..Default::default()
            }
        },
        label: None,
        memory_hints: Default::default(),
        trace: wgpu::Trace::Off,
        experimental_features: Default::default(),
    }))
    .unwrap();

    // Prepare winit
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    // Prepare & run app
    let mut app = App::new(instance, adapter, device, queue, gpu_timestamps_enabled);
    event_loop.run_app(&mut app).unwrap();
}

struct App<'a> {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    mir: Mir,
    ctx: Context,
    props: Props,
    auto_dj_1: Option<AutoDJ>,
    auto_dj_2: Option<AutoDJ>,
    auto_dj_1_enabled: bool,
    auto_dj_2_enabled: bool,
    autosave_timer: usize,
    preview_render_target: (RenderTargetId, RenderTarget),
    waveform_texture: Option<egui::TextureId>,
    spectrum_texture: Option<egui::TextureId>,
    beat_texture: Option<egui::TextureId>,
    left_panel_expanded: bool,
    library_newly_opened: bool,
    insertion_point: InsertionPoint,
    selected_device_name: String,
    available_devices: Vec<AudioInputDevice>,
    selected_system_source_name: String,
    available_system_sources: Vec<SystemAudioSource>,
    frame_index: usize,
    last_preview_paint_results: HashMap<NodeId, ArcTextureViewSampler>,
    last_ui_bg_paint_results: HashMap<NodeId, ArcTextureViewSampler>,
    last_ui_bg_render_target_id: Option<RenderTargetId>,
    fallback_preview_texture: ArcTextureViewSampler,
    preview_images: HashMap<NodeId, egui::TextureId>,
    winit_output: WinitOutput<'a>,
    perf_reporter: Option<PerfReporter>,
    gpu_timestamp_profiler: Option<GpuTimestampProfiler>,
    app_ui: Option<AppUi>, // Stuff we can't make until we have a window
    _sleep_guard: Option<keepawake::KeepAwake>,
}

struct AppUi {
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    window: Arc<winit::window::Window>,
    surface_config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface<'static>,
    logo_texture: egui::TextureHandle,
    waveform_widget: WaveformWidget,
    spectrum_widget: SpectrumWidget,
    beat_widget: BeatWidget,
    ui_bg: UiBg,
    can_draw: bool,
}

#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BgUniforms {
    opacity: f32,
}

impl App<'_> {
    fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        gpu_timestamps_enabled: bool,
    ) -> Self {
        let resource_dir = directories::ProjectDirs::from("", "", "Radiance")
            .unwrap()
            .data_local_dir()
            .to_owned();

        if !resource_dir.exists() {
            fs::create_dir_all(&resource_dir).expect("Failed to create resource directory");
        }

        println!("Resource directory is: {}", resource_dir.display());

        load_default_library(&resource_dir);

        let available_devices = Mir::available_input_devices();
        let available_system_sources = available_system_audio_sources();
        let app_settings = read_app_settings(&resource_dir).unwrap_or_else(|err_string| {
            println!("Failed to read app settings ({})", err_string);
            AppSettings::default()
        });
        let restored_device_name = restore_sync_input_device(&app_settings, &available_devices);

        // RADIANCE, WOO

        // Make a Mir
        let mir = Mir::new_with_device(restored_device_name);
        let selected_device_name = selected_sync_input_label(mir.selected_device_name_option());

        // Make context
        let ctx = Context::new(resource_dir.clone(), &device, &queue);

        let read_autosave_file = || {
            let contents = read_to_string(resource_dir.join(AUTOSAVE_FILENAME))
                .map_err(|e| format!("{:?}", e))?;
            serde_json::from_str(contents.as_str()).map_err(|e| format!("{:?}", e))
        };

        let props = read_autosave_file().unwrap_or_else(|err_string| {
            println!("Failed to read autosave file ({})", err_string);

            // Make a graph
            let node1_id: NodeId =
                serde_json::from_value(json!("node_TW+qCFNoz81wTMca9jRIBg")).unwrap();
            let node2_id: NodeId =
                serde_json::from_value(json!("node_IjPuN2HID3ydxcd4qOsCuQ")).unwrap();
            let node3_id: NodeId =
                serde_json::from_value(json!("node_mW00lTCmDH/03tGyNv3iCQ")).unwrap();
            let node4_id: NodeId =
                serde_json::from_value(json!("node_EdpVLI4KG5JEBRNSgKUzsw")).unwrap();
            let node5_id: NodeId =
                serde_json::from_value(json!("node_I6AAXBaZKvSUfArs2vBr4A")).unwrap();
            let node6_id: NodeId =
                serde_json::from_value(json!("node_I6AAXBaZKvSUfAxs2vBr4A")).unwrap();
            let output_node_id: NodeId =
                serde_json::from_value(json!("node_KSvPLGkiJDT+3FvPLf9JYQ")).unwrap();
            serde_json::from_value(json!({
                "graph": {
                    "nodes": [
                        node1_id,
                        node2_id,
                        node3_id,
                        node4_id,
                        node5_id,
                        node6_id,
                        output_node_id,
                    ],
                    "edges": [
                        {
                            "from": node1_id,
                            "to": node2_id,
                            "input": 0,
                        },
                        {
                            "from": node2_id,
                            "to": node5_id,
                            "input": 1,
                        },
                        {
                            "from": node3_id,
                            "to": node4_id,
                            "input": 0,
                        },
                        {
                            "from": node4_id,
                            "to": node5_id,
                            "input": 0,
                        },
                        {
                            "from": node5_id,
                            "to": output_node_id,
                            "input": 0,
                        },
                        {
                            "from": node6_id,
                            "to": node1_id,
                            "input": 0,
                        },
                    ],
                },
                "node_props": {
                    node1_id.to_string(): {
                        "type": "EffectNode",
                        "name": "purple",
                        "input_count": 1,
                        "intensity": 1.0,
                    },
                    node2_id.to_string(): {
                        "type": "EffectNode",
                        "name": "droste",
                        "input_count": 1,
                        "intensity": 1.0,
                    },
                    node3_id.to_string(): {
                        "type": "EffectNode",
                        "name": "wwave",
                        "input_count": 1,
                        "intensity": 0.6,
                        "frequency": 0.25,
                    },
                    node4_id.to_string(): {
                        "type": "EffectNode",
                        "name": "zoomin",
                        "input_count": 1,
                        "intensity": 0.3,
                        "frequency": 1.0
                    },
                    node5_id.to_string(): {
                        "type": "EffectNode",
                        "name": "uvmap",
                        "input_count": 2,
                        "intensity": 0.2,
                        "frequency": 0.0
                    },
                    node6_id.to_string(): {
                        "type": "ImageNode",
                        "name": "logo.png",
                        "intensity": 1.0,
                    },
                    output_node_id.to_string(): {
                        "type": "UiBgNode",
                        "opacity": 0.2,
                    }
                },
                "time": 0.,
                "dt": 0.03,
            }))
            .unwrap()
        });

        println!("Props: {}", serde_json::to_string(&props).unwrap());

        // Make render targets
        let preview_render_target = (
            serde_json::from_value(json!("rt_LVrjzxhXrGU7SqFo+85zkw")).unwrap(),
            RenderTarget::new(256, 256, 1. / 60.),
        );

        let fallback_preview_texture = {
            let texture_size = wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Fallback preview texture"),
                size: texture_size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[0, 0, 0, 255],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                texture_size,
            );
            let view = texture.create_view(&Default::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            ArcTextureViewSampler::new(texture, view, sampler)
        };

        let winit_output = WinitOutput::new(&device);

        let sleep_guard = keepawake::Builder::default()
            .display(true)
            .idle(true)
            .reason("Radiance is rendering live visuals")
            .app_name("Radiance")
            .create()
            .map_err(|e| println!("Failed to inhibit sleep: {}", e))
            .ok();
        let gpu_timestamp_profiler = gpu_timestamps_enabled
            .then(|| GpuTimestampProfiler::new(&device, &queue, perf_log_interval_frames()));

        App {
            instance,
            adapter,
            device,
            queue,
            mir,
            ctx,
            props,
            auto_dj_1: None,
            auto_dj_2: None,
            auto_dj_1_enabled: false,
            auto_dj_2_enabled: false,
            autosave_timer: 0,
            preview_render_target,
            waveform_texture: None,
            spectrum_texture: None,
            beat_texture: None,
            left_panel_expanded: false,
            library_newly_opened: false,
            insertion_point: Default::default(),
            selected_device_name,
            available_devices,
            selected_system_source_name: DEFAULT_SYSTEM_SOURCE_LABEL.to_owned(),
            available_system_sources,
            frame_index: 0,
            last_preview_paint_results: HashMap::new(),
            last_ui_bg_paint_results: HashMap::new(),
            last_ui_bg_render_target_id: None,
            fallback_preview_texture,
            preview_images: Default::default(),
            winit_output,
            perf_reporter: PerfReporter::from_env(),
            gpu_timestamp_profiler,
            app_ui: None,
            _sleep_guard: sleep_guard,
        }
    }

    fn finish_frame(
        &mut self,
        frame_start: Instant,
        mut sample: FramePerformanceSample,
        did_vsync: bool,
    ) -> bool {
        sample.total = frame_start.elapsed();
        sample.did_vsync = did_vsync;
        if let Some(perf_reporter) = &mut self.perf_reporter {
            perf_reporter.push(sample);
        }
        did_vsync
    }

    fn preview_cache_stale(&self) -> bool {
        self.last_preview_paint_results.len() != self.props.graph.nodes.len()
            || self
                .props
                .graph
                .nodes
                .iter()
                .any(|node_id| !self.last_preview_paint_results.contains_key(node_id))
    }

    fn refresh_preview_paint_cache(
        &mut self,
        radiance_paint_results: &HashMap<NodeId, ArcTextureViewSampler>,
    ) {
        self.last_preview_paint_results
            .retain(|node_id, _| self.props.graph.nodes.contains(node_id));

        for node_id in self.props.graph.nodes.iter().copied() {
            if let Some(texture) = radiance_paint_results.get(&node_id) {
                self.last_preview_paint_results.insert(node_id, texture.clone());
            } else {
                self.last_preview_paint_results
                    .entry(node_id)
                    .or_insert_with(|| self.fallback_preview_texture.clone());
            }
        }
    }

    fn update_preview_textures(
        &mut self,
        preview_paint_results: &HashMap<NodeId, ArcTextureViewSampler>,
    ) {
        let graph_nodes = self.props.graph.nodes.clone();
        let preview_sources: Vec<_> = graph_nodes
            .iter()
            .map(|node_id| {
                let texture = preview_paint_results
                    .get(node_id)
                    .unwrap_or(&self.fallback_preview_texture);
                (*node_id, texture.view.clone())
            })
            .collect();
        let stale_preview_nodes: Vec<_> = self
            .preview_images
            .keys()
            .copied()
            .filter(|node_id| !graph_nodes.contains(node_id))
            .collect();
        let app_ui = self.app_ui.as_mut().unwrap();

        for (node_id, native_texture) in preview_sources {
            match self.preview_images.entry(node_id) {
                Entry::Vacant(e) => {
                    e.insert(app_ui.egui_renderer.register_native_texture(
                        &self.device,
                        native_texture.as_ref(),
                        wgpu::FilterMode::Linear,
                    ));
                }
                Entry::Occupied(e) => {
                    app_ui.egui_renderer.update_egui_texture_from_wgpu_texture(
                        &self.device,
                        native_texture.as_ref(),
                        wgpu::FilterMode::Linear,
                        *e.get(),
                    );
                }
            }
        }

        for node_id in stale_preview_nodes {
            if let Some(egui_texture_id) = self.preview_images.remove(&node_id) {
                app_ui.egui_renderer.free_texture(&egui_texture_id);
            }
        }
    }

    // returns true if present() was called (forcing vsync)
    fn update(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let frame_start = Instant::now();
        let mut did_vsync = false;
        let mut perf_sample = FramePerformanceSample::default();
        self.frame_index = self.frame_index.wrapping_add(1);
        if let Some(gpu_timestamp_profiler) = &mut self.gpu_timestamp_profiler {
            gpu_timestamp_profiler.poll(&self.device);
        }

        // Update
        let mir_poll_start = Instant::now();
        let music_info = self.mir.poll();
        perf_sample.mir_poll = mir_poll_start.elapsed();
        self.props.time = music_info.time;
        self.props.dt = music_info.tempo * (1. / 60.);
        self.props.audio = music_info.audio.clone();

        // See if we need to (re-)create the UI BG render target
        if let Some(app_ui) = &mut self.app_ui {
            let wgpu::SurfaceConfiguration { width, height, .. } = app_ui.surface_config;
            app_ui.ui_bg.create_or_update_render_target(width, height);
        }

        // Merge our render list (preview + bg) and the winit_output render list:
        let (preview_id, preview_rt) = &self.preview_render_target;
        let preview = Some((preview_id, preview_rt));
        let render_target_list = preview
            .into_iter()
            .chain(
                self.app_ui
                    .as_ref()
                    .map(|app_ui| app_ui.ui_bg.render_target())
                    .into_iter(),
            )
            .chain(self.winit_output.render_targets_iter())
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        self.auto_dj_1.as_mut().map(|a| {
            a.update(&mut self.props);

            // Uncheck the checkbox if we broke the AutoDJ
            if a.is_broken() {
                self.auto_dj_1_enabled = false;
            }
        });
        self.auto_dj_2.as_mut().map(|a| {
            a.update(&mut self.props);

            // Uncheck the checkbox if we broke the AutoDJ
            if a.is_broken() {
                self.auto_dj_2_enabled = false;
            }
        });

        let ctx_update_start = Instant::now();
        self.ctx.update(
            &self.device,
            &self.queue,
            &mut self.props,
            &render_target_list,
        );
        perf_sample.ctx_update = ctx_update_start.elapsed();

        // Autosave if necessary
        // TODO: consider moving this to a background thread
        if self.autosave_timer == 0 {
            let autosave_start = Instant::now();
            autosave(&self.ctx.resource_dir, &self.props);
            perf_sample.autosave = autosave_start.elapsed();
            self.autosave_timer = AUTOSAVE_INTERVAL_FRAMES;
        } else {
            self.autosave_timer -= 1;
        }

        let ui_bg_render_target = self
            .app_ui
            .as_ref()
            .map(|app_ui| *app_ui.ui_bg.render_target().0);
        let visible_output_count = self.winit_output.render_targets_iter().count();
        let preview_interval = if visible_output_count > 0 {
            PREVIEW_RENDER_INTERVAL_WITH_VISIBLE_OUTPUT
        } else {
            1
        };
        let ui_bg_interval = if visible_output_count > 0 {
            UI_BG_RENDER_INTERVAL_WITH_VISIBLE_OUTPUT
        } else {
            1
        };
        let preview_cache_stale = self.preview_cache_stale();
        let should_render_preview = preview_cache_stale
            || self.last_preview_paint_results.is_empty()
            || self.frame_index % preview_interval == 0;
        let should_render_ui_bg = match ui_bg_render_target {
            Some(bg_render_target_id) => {
                preview_cache_stale
                    || self.last_ui_bg_paint_results.is_empty()
                    || self.last_ui_bg_render_target_id != Some(bg_render_target_id)
                    || self.frame_index % ui_bg_interval == 0
            }
            None => false,
        };

        if ui_bg_render_target.is_none() {
            self.last_ui_bg_render_target_id = None;
            self.last_ui_bg_paint_results.clear();
        }

        let mut radiance_ui_bg_paint_results = self.last_ui_bg_paint_results.clone();
        if should_render_preview || should_render_ui_bg {
            let mut offscreen_encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Offscreen encoder"),
                });
            let mut offscreen_submission = self
                .gpu_timestamp_profiler
                .as_mut()
                .and_then(|gpu_timestamp_profiler| {
                    match (should_render_preview, should_render_ui_bg) {
                        (true, true) => gpu_timestamp_profiler
                            .begin_submission(&[GpuTimingStage::Preview, GpuTimingStage::UiBg]),
                        (true, false) => {
                            gpu_timestamp_profiler.begin_submission(&[GpuTimingStage::Preview])
                        }
                        (false, true) => {
                            gpu_timestamp_profiler.begin_submission(&[GpuTimingStage::UiBg])
                        }
                        (false, false) => None,
                    }
                });

            let mut gpu_stage_index = 0usize;
            if should_render_preview {
                if let (Some(gpu_timestamp_profiler), Some(submission)) = (
                    self.gpu_timestamp_profiler.as_mut(),
                    offscreen_submission.as_ref(),
                ) {
                    gpu_timestamp_profiler
                        .write_stage_start(&mut offscreen_encoder, submission, gpu_stage_index);
                }

                let preview_paint_start = Instant::now();
                let (preview_render_target_id, _) = self.preview_render_target;
                let radiance_preview_paint_results = self.ctx.paint(
                    &self.device,
                    &self.queue,
                    &mut offscreen_encoder,
                    preview_render_target_id,
                );
                perf_sample.graph_paints += 1;
                perf_sample.preview_paint = preview_paint_start.elapsed();

                if let (Some(gpu_timestamp_profiler), Some(submission)) = (
                    self.gpu_timestamp_profiler.as_mut(),
                    offscreen_submission.as_ref(),
                ) {
                    gpu_timestamp_profiler
                        .write_stage_end(&mut offscreen_encoder, submission, gpu_stage_index);
                }

                self.refresh_preview_paint_cache(&radiance_preview_paint_results);
                let preview_paint_cache = self.last_preview_paint_results.clone();
                self.update_preview_textures(&preview_paint_cache);
                gpu_stage_index += 1;
            }

            if should_render_ui_bg {
                if let (Some(gpu_timestamp_profiler), Some(submission)) = (
                    self.gpu_timestamp_profiler.as_mut(),
                    offscreen_submission.as_ref(),
                ) {
                    gpu_timestamp_profiler
                        .write_stage_start(&mut offscreen_encoder, submission, gpu_stage_index);
                }

                let ui_bg_paint_start = Instant::now();
                let bg_render_target_id = ui_bg_render_target.unwrap();
                radiance_ui_bg_paint_results = self.ctx.paint(
                    &self.device,
                    &self.queue,
                    &mut offscreen_encoder,
                    bg_render_target_id,
                );
                perf_sample.graph_paints += 1;
                perf_sample.ui_bg_paint = ui_bg_paint_start.elapsed();

                if let (Some(gpu_timestamp_profiler), Some(submission)) = (
                    self.gpu_timestamp_profiler.as_mut(),
                    offscreen_submission.as_ref(),
                ) {
                    gpu_timestamp_profiler
                        .write_stage_end(&mut offscreen_encoder, submission, gpu_stage_index);
                }

                self.last_ui_bg_render_target_id = Some(bg_render_target_id);
                self.last_ui_bg_paint_results = radiance_ui_bg_paint_results.clone();
            }

            if let (Some(gpu_timestamp_profiler), Some(submission)) = (
                self.gpu_timestamp_profiler.as_mut(),
                offscreen_submission.as_ref(),
            ) {
                gpu_timestamp_profiler.encode_resolve(&mut offscreen_encoder, submission);
            }

            let offscreen_command_buffer = offscreen_encoder.finish();
            if let (Some(gpu_timestamp_profiler), Some(submission)) = (
                self.gpu_timestamp_profiler.as_mut(),
                offscreen_submission.take(),
            ) {
                gpu_timestamp_profiler.schedule_readback(&offscreen_command_buffer, submission);
            }
            self.queue.submit(iter::once(offscreen_command_buffer));
            perf_sample.queue_submits += 1;
        }

        // Run the UI
        let ui_cpu_start = Instant::now();
        {
            let Some(app_ui) = &mut self.app_ui else {
                return self.finish_frame(frame_start, perf_sample, did_vsync);
            };
            let raw_input = app_ui.egui_state.take_egui_input(&app_ui.window);
            app_ui.egui_ctx.begin_pass(raw_input);
        }
        self.ui(&music_info);

        let app_ui = self.app_ui.as_mut().unwrap();
        let full_output = app_ui.egui_ctx.end_pass();

        app_ui
            .egui_state
            .handle_platform_output(&app_ui.window, full_output.platform_output);
        perf_sample.ui_cpu = ui_cpu_start.elapsed();

        // Construct or destroy the AutoDJs
        match (self.auto_dj_1_enabled, &mut self.auto_dj_1) {
            (false, Some(_)) => {
                self.auto_dj_1 = None;
            }
            (true, None) => {
                self.auto_dj_1 = Some(AutoDJ::new());
            }
            _ => {}
        }
        match (self.auto_dj_2_enabled, &mut self.auto_dj_2) {
            (false, Some(_)) => {
                self.auto_dj_2 = None;
            }
            (true, None) => {
                self.auto_dj_2 = Some(AutoDJ::new());
            }
            _ => {}
        }

        // Update & paint other windows
        let output_update_stats = self.winit_output.update(
            event_loop,
            &mut self.ctx,
            &mut self.props,
            &self.instance,
            &self.adapter,
            &self.device,
            &self.queue,
            self.gpu_timestamp_profiler.as_mut(),
        );
        perf_sample.absorb_output_update(&output_update_stats);
        if output_update_stats.did_vsync {
            did_vsync = true;
        }

        let ui_gpu_start = Instant::now();
        app_ui.ui_bg.update(
            &self.device,
            &self.queue,
            &self.props,
            &radiance_ui_bg_paint_results,
        );

        // UI GPU update
        let tris = app_ui
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            app_ui
                .egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [app_ui.surface_config.width, app_ui.surface_config.height],
            pixels_per_point: app_ui.window.scale_factor() as f32,
        };

        // See if we can present (window is not occluded)
        // (on Mac, we can always draw.) XXX test this
        // (on Windows, we can always draw, and in fact, the redraw signal doesn't fire as it should.)
        if cfg!(target_os = "linux") && !app_ui.can_draw {
            return did_vsync;
        }
        app_ui.can_draw = false;
        app_ui.window.request_redraw();

        let output = app_ui.surface.get_current_texture().unwrap();
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });
        let mut ui_submission = self
            .gpu_timestamp_profiler
            .as_mut()
            .and_then(|gpu_timestamp_profiler| {
                gpu_timestamp_profiler.begin_submission(&[GpuTimingStage::Ui])
            });
        if let (Some(gpu_timestamp_profiler), Some(submission)) = (
            self.gpu_timestamp_profiler.as_mut(),
            ui_submission.as_ref(),
        ) {
            gpu_timestamp_profiler.write_stage_start(&mut encoder, submission, 0);
        }
        app_ui.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &tris,
            &screen_descriptor,
        );
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(from_srgb(25, 25, 25, 255)),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            perf_sample.surface_passes += 1;

            // Draw background
            app_ui.ui_bg.render(&self.device, &mut render_pass);

            // Draw EGUI
            app_ui.egui_renderer.render(
                &mut render_pass.forget_lifetime(),
                &tris,
                &screen_descriptor,
            );
        }
        if let (Some(gpu_timestamp_profiler), Some(submission)) = (
            self.gpu_timestamp_profiler.as_mut(),
            ui_submission.as_ref(),
        ) {
            gpu_timestamp_profiler.write_stage_end(&mut encoder, submission, 0);
            gpu_timestamp_profiler.encode_resolve(&mut encoder, submission);
        }
        let ui_command_buffer = encoder.finish();
        if let (Some(gpu_timestamp_profiler), Some(submission)) = (
            self.gpu_timestamp_profiler.as_mut(),
            ui_submission.take(),
        ) {
            gpu_timestamp_profiler.schedule_readback(&ui_command_buffer, submission);
        }
        self.queue.submit(std::iter::once(ui_command_buffer));
        perf_sample.queue_submits += 1;
        perf_sample.ui_gpu = ui_gpu_start.elapsed();

        for id in &full_output.textures_delta.free {
            app_ui.egui_renderer.free_texture(id);
        }
        let ui_present_start = Instant::now();
        app_ui.window.pre_present_notify();
        output.present();
        perf_sample.ui_present = ui_present_start.elapsed();
        perf_sample.presented_windows += 1;
        did_vsync = true;
        self.finish_frame(frame_start, perf_sample, did_vsync)
    }

    fn ui(&mut self, music_info: &MusicInfo) {
        fn update_or_register_native_texture(
            egui_renderer: &mut egui_wgpu::Renderer,
            device: &wgpu::Device,
            native_texture: &wgpu::TextureView,
            egui_texture: &mut Option<egui::TextureId>,
        ) {
            match egui_texture {
                None => {
                    *egui_texture = Some(egui_renderer.register_native_texture(
                        device,
                        native_texture,
                        wgpu::FilterMode::Linear,
                    ));
                }
                Some(egui_texture) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        native_texture,
                        wgpu::FilterMode::Linear,
                        *egui_texture,
                    );
                }
            }
        }

        let app_ui = self.app_ui.as_mut().unwrap();

        let waveform_size = egui::vec2(330., 75.);
        let spectrum_size = egui::vec2(330., 75.);
        let beat_size = egui::vec2(75., 75.);
        {
            let stale_preview_nodes: Vec<_> = self
                .preview_images
                .keys()
                .copied()
                .filter(|node_id| !self.props.graph.nodes.contains(node_id))
                .collect();
            for node_id in stale_preview_nodes {
                if let Some(egui_texture_id) = self.preview_images.remove(&node_id) {
                    app_ui.egui_renderer.free_texture(&egui_texture_id);
                }
            }

            // Update & paint widgets

            let waveform_native_texture = app_ui.waveform_widget.paint(
                &self.device,
                &self.queue,
                waveform_size,
                &music_info.audio,
                music_info.uncompensated_unscaled_time,
            );

            update_or_register_native_texture(
                &mut app_ui.egui_renderer,
                &self.device,
                &waveform_native_texture.view,
                &mut self.waveform_texture,
            );

            let spectrum_native_texture = app_ui.spectrum_widget.paint(
                &self.device,
                &self.queue,
                spectrum_size,
                &music_info.spectrum,
            );

            update_or_register_native_texture(
                &mut app_ui.egui_renderer,
                &self.device,
                &spectrum_native_texture.view,
                &mut self.spectrum_texture,
            );

            let beat_native_texture = app_ui.beat_widget.paint(
                &self.device,
                &self.queue,
                beat_size,
                music_info.unscaled_time,
            );

            update_or_register_native_texture(
                &mut app_ui.egui_renderer,
                &self.device,
                &beat_native_texture.view,
                &mut self.beat_texture,
            );
        }

        let left_panel_response = egui::SidePanel::left("left").show_animated(
            &app_ui.egui_ctx,
            self.left_panel_expanded,
            |ui| library::library_ui(ui, &self.ctx, self.library_newly_opened),
        );

        let full_rect = app_ui.egui_ctx.available_rect();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(&app_ui.egui_ctx, |ui| {
                let modal_id = ui.make_persistent_id("modal");
                let modal_shown = modal_shown(&app_ui.egui_ctx, modal_id);

                // Mosaic
                let egui::InnerResponse {
                    inner: mosaic_response,
                    ..
                } = ui.scope_builder(
                    {
                        let mut builder = egui::UiBuilder::default().max_rect(full_rect);
                        builder.disabled = modal_shown;
                        builder
                    },
                    |ui| {
                        let egui::containers::scroll_area::ScrollAreaOutput {
                            inner: mosaic_response,
                            ..
                        } = egui::containers::scroll_area::ScrollArea::both()
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.add(mosaic(
                                    "mosaic",
                                    &mut self.props,
                                    self.ctx.node_states(),
                                    &self.preview_images,
                                    &mut self.insertion_point,
                                    modal_id,
                                ))
                            });
                        mosaic_response
                    },
                );

                // Top bar
                ui.scope_builder(
                    {
                        let top_bar_rect = egui::Rect::from_min_size(
                            full_rect.min,
                            egui::vec2(full_rect.width(), 80.0),
                        );
                        let mut builder = egui::UiBuilder::default().max_rect(top_bar_rect);
                        builder.disabled = modal_shown;
                        builder
                    },
                    |ui| {
                        egui::Frame::NONE
                            .fill(egui::Color32::from_rgba_premultiplied(25, 25, 25, 250))
                            .show(ui, |ui| {
                                ui.horizontal_centered(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add_space(5.0);
                                    ui.add(
                                        egui::Image::new((app_ui.logo_texture.id(), LOGO_SIZE))
                                            .fit_to_exact_size(LOGO_SIZE),
                                    );
                                    ui.add_space(5.0);
                                    ui.image((self.waveform_texture.unwrap(), waveform_size));
                                    ui.image((self.spectrum_texture.unwrap(), spectrum_size));
                                    ui.image((self.beat_texture.unwrap(), beat_size));
                                    ui.label("Beat lock:");
                                    let beat_lock_text = if music_info.beat_locked {
                                        "Locked"
                                    } else {
                                        "No lock"
                                    };
                                    let beat_lock_color = if music_info.beat_locked {
                                        egui::Color32::from_rgb(94, 201, 126)
                                    } else {
                                        egui::Color32::from_rgb(230, 184, 79)
                                    };
                                    ui.colored_label(beat_lock_color, beat_lock_text).on_hover_text(
                                        "Locked means the incoming audio is driving beat time. No lock means Radiance is still running on fallback tempo until it finds stable beats.",
                                    );
                                    ui.checkbox(&mut self.auto_dj_1_enabled, "Auto DJ 1");
                                    ui.checkbox(&mut self.auto_dj_2_enabled, "Auto DJ 2");

                                    ui.label("Global timescale:");
                                    let timescales: &[f32] = &[0.125, 0.25, 0.5, 1., 2., 4., 8.];
                                    fn str_for_timescale(timescale: f32) -> String {
                                        if timescale < 1. {
                                            format!("{}x slower", 1. / timescale)
                                        } else if timescale == 1. {
                                            "1x".to_owned()
                                        } else if timescale > 1. {
                                            format!("{}x faster", timescale)
                                        } else {
                                            format!("{}", timescale)
                                        }
                                    }
                                    egui::ComboBox::from_id_salt("global timescale")
                                        .selected_text(
                                            str_for_timescale(self.mir.global_timescale).as_str(),
                                        )
                                        .show_ui(ui, |ui| {
                                            for &timescale in timescales.iter() {
                                                ui.selectable_value(
                                                    &mut self.mir.global_timescale,
                                                    timescale,
                                                    str_for_timescale(timescale).as_str(),
                                                );
                                            }
                                        });
                                    ui.label("Latency compensation:");
                                    ui.add(
                                        egui::DragValue::new(&mut self.mir.latency_compensation)
                                            .speed(0.001)
                                            .fixed_decimals(3)
                                            .suffix("s")
                                            .range(0. ..=1.),
                                    );
                                    ui.label("Sync input:");
                                    let prev_device = self.selected_device_name.clone();
                                    let combo = egui::ComboBox::from_id_salt("audio input device")
                                        .selected_text(self.selected_device_name.as_str())
                                        .show_ui(ui, |ui| {
                                            let likely_microphones: Vec<_> = self
                                                .available_devices
                                                .iter()
                                                .filter(|device| {
                                                    device.kind == InputDeviceKind::Microphone
                                                })
                                                .collect();
                                            let likely_loopback_inputs: Vec<_> = self
                                                .available_devices
                                                .iter()
                                                .filter(|device| {
                                                    device.kind == InputDeviceKind::Loopback
                                                })
                                                .collect();
                                            let other_inputs: Vec<_> = self
                                                .available_devices
                                                .iter()
                                                .filter(|device| device.kind == InputDeviceKind::Other)
                                                .collect();

                                            ui.selectable_value(
                                                &mut self.selected_device_name,
                                                DEFAULT_AUDIO_INPUT_LABEL.to_owned(),
                                                DEFAULT_AUDIO_INPUT_LABEL,
                                            );
                                            show_sync_input_group(
                                                ui,
                                                &mut self.selected_device_name,
                                                "Likely microphones",
                                                &likely_microphones,
                                            );
                                            show_sync_input_group(
                                                ui,
                                                &mut self.selected_device_name,
                                                "Likely loopback / monitor inputs",
                                                &likely_loopback_inputs,
                                            );
                                            show_sync_input_group(
                                                ui,
                                                &mut self.selected_device_name,
                                                "Other inputs",
                                                &other_inputs,
                                            );
                                        });
                                    if combo.response.clicked() {
                                        self.available_devices = Mir::available_input_devices();
                                    }
                                    if self.selected_device_name != prev_device {
                                        let device = if self.selected_device_name
                                            == DEFAULT_AUDIO_INPUT_LABEL
                                        {
                                            None
                                        } else {
                                            Some(self.selected_device_name.clone())
                                        };
                                        self.mir.switch_device(device);
                                        self.selected_device_name =
                                            selected_sync_input_label(self.mir.selected_device_name_option());
                                        write_app_settings(
                                            &self.ctx.resource_dir,
                                            &AppSettings {
                                                sync_input_device_name: self
                                                    .mir
                                                    .selected_device_name_option()
                                                    .map(str::to_owned),
                                            },
                                        );
                                    }
                                    if !self.available_system_sources.is_empty() {
                                        ui.label("System source:");
                                        let prev_system_source =
                                            self.selected_system_source_name.clone();
                                        let system_source_combo = egui::ComboBox::from_id_salt(
                                            "system audio source",
                                        )
                                        .selected_text(selected_system_source_label(
                                            self.selected_system_source_name.as_str(),
                                            &self.available_system_sources,
                                        ))
                                        .show_ui(ui, |ui| {
                                            let monitor_sources: Vec<_> = self
                                                .available_system_sources
                                                .iter()
                                                .filter(|source| source.is_monitor)
                                                .collect();
                                            let other_sources: Vec<_> = self
                                                .available_system_sources
                                                .iter()
                                                .filter(|source| !source.is_monitor)
                                                .collect();

                                            ui.selectable_value(
                                                &mut self.selected_system_source_name,
                                                DEFAULT_SYSTEM_SOURCE_LABEL.to_owned(),
                                                DEFAULT_SYSTEM_SOURCE_LABEL,
                                            );
                                            show_system_source_group(
                                                ui,
                                                &mut self.selected_system_source_name,
                                                "Monitor / loopback sources",
                                                &monitor_sources,
                                            );
                                            show_system_source_group(
                                                ui,
                                                &mut self.selected_system_source_name,
                                                "Other system sources",
                                                &other_sources,
                                            );
                                        });
                                        if system_source_combo.response.clicked() {
                                            self.available_system_sources =
                                                available_system_audio_sources();
                                        }
                                        if self.selected_system_source_name != prev_system_source {
                                            let requested_source = if self.selected_system_source_name
                                                == DEFAULT_SYSTEM_SOURCE_LABEL
                                            {
                                                None
                                            } else {
                                                Some(self.selected_system_source_name.as_str())
                                            };
                                            match move_radiance_capture_to_system_source(
                                                requested_source,
                                            ) {
                                                Ok(actual_source_name) => {
                                                    self.selected_system_source_name =
                                                        actual_source_name.unwrap_or_else(|| {
                                                            DEFAULT_SYSTEM_SOURCE_LABEL.to_owned()
                                                        });
                                                }
                                                Err(err) => {
                                                    println!(
                                                        "Failed to switch Radiance system source: {}",
                                                        err
                                                    );
                                                    self.selected_system_source_name =
                                                        prev_system_source;
                                                }
                                            }
                                        }
                                    }
                                });
                            });

                        if !self.left_panel_expanded && ui.input(|i| i.key_pressed(egui::Key::A)) {
                            self.left_panel_expanded = true;
                            self.library_newly_opened = true;
                        }
                        if self.left_panel_expanded
                            && ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            self.left_panel_expanded = false;
                            self.library_newly_opened = false;
                        }

                        if let Some(egui::InnerResponse {
                            inner: library_response,
                            response: _,
                        }) = left_panel_response
                        {
                            // Reset the focus flag after it's been used
                            self.library_newly_opened = false;

                            match library_response {
                                library::LibraryResponse::AddNode(node_props) => {
                                    let new_node_id = NodeId::gen();
                                    self.props.node_props.insert(new_node_id, node_props);
                                    self.props
                                        .graph
                                        .insert_node(new_node_id, &self.insertion_point);
                                    self.left_panel_expanded = false;
                                    mosaic_response.request_focus();
                                }
                                library::LibraryResponse::Close => {
                                    self.left_panel_expanded = false;
                                    mosaic_response.request_focus();
                                }
                                library::LibraryResponse::None => {}
                            }
                        }
                    },
                );

                // Library toggle button
                let arrow_icon = if self.left_panel_expanded {
                    "\u{23F4}"
                } else {
                    "\u{23F5}"
                };
                let button_rect = egui::Rect::from_min_size(
                    egui::pos2(full_rect.left(), full_rect.top() + 80.),
                    egui::vec2(20.0, 80.0),
                );
                if ui
                    .place(button_rect, egui::Button::new(arrow_icon))
                    .clicked()
                {
                    self.left_panel_expanded = !self.left_panel_expanded;
                    self.library_newly_opened = self.left_panel_expanded;
                }

                if modal_shown {
                    ui.scope_builder(egui::UiBuilder::default().max_rect(full_rect), |ui| {
                        ui.add(modal(
                            modal_id,
                            &mut self.props,
                            self.ctx.node_states(),
                            &self.preview_images,
                        ));
                    });
                }
            });
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        let Some(app_ui) = &mut self.app_ui else {
            return;
        };
        if new_size.width > 0 && new_size.height > 0 {
            app_ui.surface_config.width = new_size.width;
            app_ui.surface_config.height = new_size.height;
            app_ui
                .surface
                .configure(&self.device, &app_ui.surface_config);
        }
    }
}

impl AppUi {
    fn new(app: &App, window: winit::window::Window) -> Self {
        // Make egui context
        let egui_ctx = egui::Context::default();
        egui_ctx.set_theme(egui::Theme::Dark);
        egui_ctx.style_mut(|style| {
            style.interaction.selectable_labels = false;
            style.visuals.handle_shape = egui::style::HandleShape::Circle;
        });

        // Make egui state
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        let window = Arc::new(window);

        let size = window.inner_size();
        let surface = app.instance.create_surface(window.clone()).unwrap();
        let surface_caps = surface.get_capabilities(&app.adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&app.device, &surface_config);

        let egui_renderer = egui_wgpu::Renderer::new(
            &app.device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

        // Make widgets
        let pixels_per_point = window.scale_factor() as f32;
        let waveform_widget = WaveformWidget::new(&app.device, pixels_per_point);
        let spectrum_widget = SpectrumWidget::new(&app.device, pixels_per_point);
        let beat_widget = BeatWidget::new(&app.device, pixels_per_point);

        // Make BG
        let ui_bg = UiBg::new(&app.device, surface_format);

        // Load logo
        let logo_bytes = include_bytes!("../../library/logo.png");
        let logo_image = image::load_from_memory(logo_bytes).unwrap();

        let logo_pixel_size = LOGO_SIZE * pixels_per_point;
        let logo_pixel_w = logo_pixel_size.x as u32;
        let logo_pixel_h = logo_pixel_size.y as u32;
        let logo_resized = image::imageops::resize(
            &logo_image,
            logo_pixel_w,
            logo_pixel_h,
            image::imageops::FilterType::Lanczos3,
        );

        let logo_texture = egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [logo_pixel_w as usize, logo_pixel_h as usize],
                &logo_resized,
            ),
            Default::default(),
        );

        AppUi {
            window,
            surface_config,
            surface,
            egui_ctx,
            egui_state,
            egui_renderer,
            waveform_widget,
            spectrum_widget,
            beat_widget,
            ui_bg,
            logo_texture,
            can_draw: false,
        }
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app_ui.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("Radiance")
                .with_maximized(true);

            let window = event_loop.create_window(window_attributes).unwrap();
            self.app_ui = Some(AppUi::new(&self, window));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(app_ui) = &mut self.app_ui else {
            return;
        };

        if self
            .winit_output
            .window_event(window_id, &event, &self.device)
        {
            // Event handled by another window
            return;
        }

        if window_id != app_ui.window.id() {
            return;
        }

        let response = app_ui.egui_state.on_window_event(&app_ui.window, &event);

        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                // This assignment prevents the app from segfaulting on exit
                // I think this is a bug in winit that may be fixed in a future version.
                self.app_ui = None;
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                self.resize(physical_size);
            }
            WindowEvent::RedrawRequested => {
                app_ui.can_draw = true;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let did_vsync = self.update(event_loop);
        if did_vsync {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            // If we didn't vsync as part of rendering (e.g. all windows occluded,)
            // fall back to a 60 FPS rate timed on the CPU
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_secs_f64(1. / 60.),
            ));
        }
    }
}

// From https://github.com/three-rs/three/blob/07e47da5e0673aa9a16526719e16debd59040eec/src/color.rs#L39
fn from_srgb(r: u8, g: u8, b: u8, a: u8) -> wgpu::Color {
    let f = |xu| {
        let x = xu as f64 / 255.0;
        if x > 0.04045 {
            ((x + 0.055) / 1.055).powf(2.4)
        } else {
            x / 12.92
        }
    };
    wgpu::Color {
        r: f(r),
        g: f(g),
        b: f(b),
        a: f(a),
    }
}
