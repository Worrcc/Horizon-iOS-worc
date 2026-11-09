//! iOS runtime overrides and ten-second, in-world diagnostics.
use super::Game;
use horizon_ios::{ThermalState, thermal::Governor};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct PhonePerf {
    governor: Governor,
    thermal: ThermalState,
    low_power: bool,
    scale: Option<u32>,
    cap: Option<i32>,
    log: PerfLog,
}

impl PhonePerf {
    pub(super) fn suspend(&mut self) {
        self.log = PerfLog::default();
        // Time in the background cannot prove uninterrupted cooling.
        self.governor.update(self.governor.state, Duration::ZERO);
    }
}

impl Game<'_> {
    pub(super) fn update_phone_performance(&mut self) {
        let p = &mut self.phone_perf;
        p.thermal = horizon_ios::thermal_state();
        p.low_power = horizon_ios::low_power_mode();
        if self.settings.horizon.thermal_throttling {
            p.governor.update(p.thermal, self.started.elapsed());
        } else {
            p.governor = Governor::default();
        }
        let scale = p
            .governor
            .resolution_scale(self.settings.horizon.resolution_scale);
        if p.scale != Some(scale)
            && let Some(r) = &mut self.renderer
        {
            let g = &self.settings.horizon;
            r.set_world_post(scale as f32 / 100.0, g.fxaa, g.taa, g.upscaling.ratio());
            p.scale = Some(scale);
        }
    }

    pub(super) fn phone_resolution_scale(&self) -> u32 {
        self.phone_perf
            .governor
            .resolution_scale(self.settings.horizon.resolution_scale)
    }

    pub(super) fn phone_frame_cap(&self, limit: i32) -> i32 {
        self.phone_perf.governor.frame_cap(limit)
    }

    pub(super) fn phone_display_cap(&mut self, cap: i32) {
        if self.phone_perf.cap != Some(cap) {
            horizon_ios::prefer_frame_rate(cap as f32);
            self.phone_perf.cap = Some(cap);
        }
    }

    pub(super) fn log_phone_performance(
        &mut self,
        rendered: bool,
        elapsed: Duration,
        cpu: Duration,
    ) {
        if self.title.is_some() || self.player.is_none() || self.remote_loading() {
            self.phone_perf.log = PerfLog::default();
            return;
        }
        if !rendered {
            return;
        }
        let cap = self.framerate_limit();
        let p = &mut self.phone_perf;
        let gpu = self
            .renderer
            .as_ref()
            .map_or(&[][..], |r| r.gpu_times.as_slice());
        let Some(stats) = p.log.record(Instant::now(), elapsed, cpu, gpu) else {
            return;
        };
        let gpu = stats
            .gpu_ms
            .map_or_else(|| "n/a".into(), |ms| format!("{ms:.2}"));
        let memory = horizon_threads::process_memory::resident_bytes().map_or_else(
            || "n/a".into(),
            |bytes| format!("{:.1}", bytes as f64 / 1048576.0),
        );
        let temperature = horizon_ios::battery_temperature_c()
            .map_or_else(String::new, |degrees| {
                format!(" battery_temperature_c={degrees:.1}")
            });
        log::info!(
            "perf: avg_fps={:.1} low_1pct_ms={:.2} low_0.1pct_ms={:.2} cpu_ms={:.2} gpu_ms={} render_distance={} resolution_scale={} fps_cap={} thermal={:?}{} low_power={} governor={:?} governor_enabled={} resident_mib={}",
            stats.fps,
            stats.low_1,
            stats.low_01,
            stats.cpu_ms,
            gpu,
            self.settings.render_distance,
            p.governor
                .resolution_scale(self.settings.horizon.resolution_scale),
            cap,
            p.thermal,
            temperature,
            p.low_power,
            p.governor.state,
            self.settings.horizon.thermal_throttling,
            memory,
        );
    }
}

#[derive(Default)]
struct PerfLog {
    start: Option<Instant>,
    frames: Vec<f64>,
    cpu_ms: f64,
    gpu_ms: f64,
    gpu_samples: usize,
}
struct Stats {
    fps: f64,
    low_1: f64,
    low_01: f64,
    cpu_ms: f64,
    gpu_ms: Option<f64>,
}
impl PerfLog {
    fn record(
        &mut self,
        now: Instant,
        frame: Duration,
        cpu: Duration,
        gpu: &[f32],
    ) -> Option<Stats> {
        // Ignore the first interval: it may include loading or time spent in menus.
        let Some(start) = self.start else {
            self.start = Some(now);
            return None;
        };
        self.frames.push(frame.as_secs_f64() * 1000.0);
        self.cpu_ms += cpu.as_secs_f64() * 1000.0;
        self.gpu_ms += gpu.iter().map(|ms| f64::from(*ms)).sum::<f64>();
        self.gpu_samples += gpu.len();
        if now.duration_since(start) < Duration::from_secs(10) {
            return None;
        }
        self.frames.sort_unstable_by(|a, b| b.total_cmp(a));
        let count = self.frames.len();
        // Mean of the slowest 1% / 0.1%, expressed in milliseconds (larger is worse).
        let slow_mean = |divisor: usize| {
            let n = count.div_ceil(divisor);
            self.frames[..n].iter().sum::<f64>() / n as f64
        };
        let stats = Stats {
            fps: count as f64 * 1000.0 / self.frames.iter().sum::<f64>(),
            low_1: slow_mean(100),
            low_01: slow_mean(1000),
            cpu_ms: self.cpu_ms / count as f64,
            gpu_ms: (self.gpu_samples > 0).then(|| self.gpu_ms / self.gpu_samples as f64),
        };
        self.frames.clear();
        self.start = Some(now);
        self.cpu_ms = 0.0;
        self.gpu_ms = 0.0;
        self.gpu_samples = 0;
        Some(stats)
    }
}
