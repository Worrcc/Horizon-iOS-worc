//! iOS runtime overrides and ten-second, in-world diagnostics.
#[cfg(target_os = "ios")]
use super::Game;
#[cfg(target_os = "ios")]
use horizon_client::lang;
use horizon_client::spans::{self, Span};
#[cfg(target_os = "ios")]
use horizon_ios::{ThermalState, thermal::Governor};
use std::time::{Duration, Instant};

/// What the phone's screen shows, for its frame cap.
#[cfg(any(target_os = "ios", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PhoneScreen {
    Gameplay,
    PausedSingleplayer,
    InWorldScreen,
    OutOfWorld,
}

/// The frame cap for a screen: the single-player pause screen 30, other screens 60,
/// gameplay as it is.
#[cfg(any(target_os = "ios", test))]
pub(super) fn phone_screen_cap(limit: i32, screen: PhoneScreen) -> i32 {
    match screen {
        PhoneScreen::Gameplay => limit,
        PhoneScreen::PausedSingleplayer => limit.min(30),
        PhoneScreen::InWorldScreen | PhoneScreen::OutOfWorld => limit.min(60),
    }
}

#[cfg(target_os = "ios")]
#[derive(Default)]
pub(super) struct PhonePerf {
    governor: Governor,
    thermal: ThermalState,
    low_power: bool,
    scale: Option<u32>,
    cap: Option<i32>,
    log: PerfLog,
    frame_at: Option<Instant>,
    usage_from: Option<(Instant, horizon_threads::process_usage::Usage)>,
    background: bool,
    offer: PhoneOffer,
}

/// The one-time offer of the phone defaults.
#[cfg(target_os = "ios")]
#[derive(Default)]
enum PhoneOffer {
    #[default]
    NotYet,
    Shown(horizon_ios::AlertId),
    Done,
}

#[cfg(target_os = "ios")]
impl PhonePerf {
    pub(super) fn suspend(&mut self) {
        self.log.suspend();
        self.frame_at = None;
        self.usage_from = None;
        // Time in the background cannot prove uninterrupted cooling.
        self.governor.update(self.governor.state, Duration::ZERO);
    }
}

#[cfg(target_os = "ios")]
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
        let scale = self.phone_scale();
        let p = &mut self.phone_perf;
        if p.scale != Some(scale)
            && let Some(r) = &mut self.renderer
        {
            let g = &self.settings.horizon;
            r.set_world_post(scale as f32 / 100.0, g.fxaa, g.taa, g.upscaling.ratio());
            p.scale = Some(scale);
        }
        self.offer_phone_defaults();
    }

    /// Offers the phone defaults once per install, on the title screen, to settings that
    /// still have the desktop defaults. Either answer is remembered.
    fn offer_phone_defaults(&mut self) {
        match self.phone_perf.offer {
            PhoneOffer::Done => {}
            PhoneOffer::NotYet => {
                if self.title.is_none()
                    || !super::account::idle()
                    || self.started.elapsed() < Duration::from_secs(3)
                {
                    return;
                }
                if !self.settings.horizon.phone_defaults_offered
                    && self.settings.phone_defaults_due()
                {
                    let id = horizon_ios::show_alert(
                        &lang::get("horizon.ios.phone_defaults.title"),
                        &lang::get("horizon.ios.phone_defaults.message"),
                        &[
                            &lang::get("horizon.ios.phone_defaults.apply"),
                            &lang::get("horizon.ios.phone_defaults.keep"),
                        ],
                    );
                    self.phone_perf.offer = PhoneOffer::Shown(id);
                } else {
                    if !self.settings.horizon.phone_defaults_offered {
                        self.settings.horizon.phone_defaults_offered = true;
                        self.settings.save(&Self::options_path());
                    }
                    self.phone_perf.offer = PhoneOffer::Done;
                }
            }
            PhoneOffer::Shown(id) => {
                let Some(answer) = horizon_ios::alert_answer(id) else {
                    return;
                };
                if answer == 0 {
                    self.settings.apply_phone_defaults();
                    log::info!("Horizon: phone defaults applied");
                }
                self.settings.horizon.phone_defaults_offered = true;
                self.settings.save(&Self::options_path());
                self.phone_perf.offer = PhoneOffer::Done;
            }
        }
    }

    pub(super) fn phone_low_power(&self) -> bool {
        self.phone_perf.low_power
    }

    pub(super) fn set_phone_background(&mut self, on: bool) {
        self.phone_perf.background = on;
    }

    pub(super) fn phone_in_background(&self) -> bool {
        self.phone_perf.background
    }

    pub(super) fn phone_resolution_scale(&self) -> u32 {
        self.phone_scale()
    }

    /// The resolution scale in force: the player's, lowered by heat, and in Low Power Mode
    /// (both follow the Thermal Throttling switch).
    fn phone_scale(&self) -> u32 {
        self.phone_perf.governor.resolution_scale_low_power(
            self.settings.horizon.resolution_scale,
            self.phone_perf.low_power && self.settings.horizon.thermal_throttling,
        )
    }

    pub(super) fn phone_frame_cap(&self, limit: i32) -> i32 {
        self.phone_perf
            .governor
            .frame_cap(phone_screen_cap(limit, self.phone_screen()))
    }

    fn phone_screen(&self) -> PhoneScreen {
        if self.title.is_some() {
            PhoneScreen::OutOfWorld
        } else if self.screen == super::Screen::Paused && self.pauses_game() {
            PhoneScreen::PausedSingleplayer
        } else if self.screen != super::Screen::Playing {
            PhoneScreen::InWorldScreen
        } else {
            PhoneScreen::Gameplay
        }
    }

    pub(super) fn phone_display_cap(&mut self, cap: i32) {
        if self.phone_perf.cap != Some(cap) {
            horizon_ios::prefer_frame_rate(cap as f32);
            self.phone_perf.cap = Some(cap);
        }
    }

    pub(super) fn log_phone_performance(&mut self, rendered: bool, cpu: Duration, spans: &[Span]) {
        let now = Instant::now();
        // End-to-end timing pairs the completed draw with the spans just taken for it.
        let elapsed = self
            .phone_perf
            .frame_at
            .replace(now)
            .map_or(Duration::ZERO, |at| now - at);
        let (frame_id, samples) = self
            .renderer
            .as_mut()
            .map_or_else(|| (None, Vec::new()), |r| r.phone_gpu_timings());
        if self.title.is_some() || self.player.is_none() || self.remote_loading() {
            self.phone_perf.log = PerfLog::default();
            self.phone_perf.usage_from = None;
            return;
        }
        if !rendered {
            return;
        }
        let cap = self.framerate_limit();
        let scale = self.phone_scale();
        let p = &mut self.phone_perf;
        let gpu = self
            .renderer
            .as_ref()
            .map_or(&[][..], |r| r.gpu_times.as_slice());
        if p.usage_from.is_none() {
            p.usage_from = horizon_threads::process_usage::sample().map(|u| (now, u));
        }
        let Some(stats) = p
            .log
            .record(now, elapsed, cpu, gpu, frame_id, &samples, spans)
        else {
            return;
        };
        let gpu = stats
            .gpu_ms
            .map_or_else(|| "n/a".into(), |ms| format!("{ms:.2}"));
        let memory = horizon_threads::process_memory::resident_bytes().map_or_else(
            || "n/a".into(),
            |bytes| format!("{:.1}", bytes as f64 / 1048576.0),
        );
        let usage = horizon_threads::process_usage::sample();
        let rates = match (p.usage_from, usage) {
            (Some((at, prev)), Some(new)) => {
                horizon_threads::process_usage::rates(&prev, &new, now - at)
            }
            _ => None,
        };
        p.usage_from = usage.map(|u| (now, u));
        let usage_fields = usage_fields(rates.as_ref());
        let worst_gpu = stats
            .worst
            .gpu_ms
            .map_or_else(|| "n/a".into(), |ms| format!("{ms:.2}"));
        log::info!(
            "perf: avg_fps={:.1} low_1pct_ms={:.2} low_0.1pct_ms={:.2} cpu_ms={:.2} gpu_ms={} render_distance={} resolution_scale={} fps_cap={} thermal={:?} low_power={} governor={:?} governor_enabled={} resident_mib={}{} slow_frames={} worst_ms={:.1} worst_gpu_ms={} worst=[{}]",
            stats.fps,
            stats.low_1,
            stats.low_01,
            stats.cpu_ms,
            gpu,
            self.settings.render_distance,
            scale,
            cap,
            p.thermal,
            p.low_power,
            p.governor.state,
            self.settings.horizon.thermal_throttling,
            memory,
            usage_fields,
            stats.slow_frames,
            stats.worst.ms,
            worst_gpu,
            stats.worst.describe(),
        );
    }
}

/// The battery fields of the `perf:` line; values the OS did not report are left out.
#[cfg(any(target_os = "ios", test))]
fn usage_fields(r: Option<&horizon_threads::process_usage::Rates>) -> String {
    let Some(r) = r else {
        return String::new();
    };
    let mut out = format!(
        " proc_cpu_ms_s={:.0} ctx_switches_s={:.0}",
        r.cpu_ms_per_s, r.context_switches_per_s
    );
    if let (Some(idle), Some(intr), Some(gpu)) = (
        r.idle_wakeups_per_s,
        r.interrupt_wakeups_per_s,
        r.gpu_ms_per_s,
    ) {
        out += &format!(" idle_wakeups_s={idle:.0} intr_wakeups_s={intr:.0} gpu_ms_s={gpu:.0}");
    }
    if let Some(mw) = r.energy_mw {
        out += &format!(" energy_mw={mw:.0}");
    }
    out
}

#[derive(Default)]
struct PerfLog {
    start: Option<Instant>,
    frames: Vec<f64>,
    cpu_ms: f64,
    gpu_ms: f64,
    gpu_samples: usize,
    slow_frames: usize,
    worst: WorstFrame,
}
struct Stats {
    fps: f64,
    low_1: f64,
    low_01: f64,
    cpu_ms: f64,
    gpu_ms: Option<f64>,
    slow_frames: usize,
    worst: WorstFrame,
}
#[derive(Default)]
struct WorstFrame {
    ms: f64,
    frame_id: Option<u64>,
    gpu_ms: Option<f32>,
    spans: Vec<Span>,
}
impl WorstFrame {
    fn describe(&self) -> String {
        self.spans
            .iter()
            .map(|s| {
                if s.name == "unspanned" {
                    format!("unspanned {:.1}", s.ms)
                } else {
                    format!("{} {:.1}/{:.1}", s.name, s.ms, s.cpu_ms)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Default)]
pub(super) struct KeyboardFrames {
    visible: bool,
    pending: Vec<(bool, Vec<f64>)>,
}
impl KeyboardFrames {
    pub(super) fn visibility(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.pending.push((visible, Vec::with_capacity(3)));
        }
    }

    pub(super) fn clear(&mut self) {
        self.pending.clear();
    }

    pub(super) fn frame(&mut self, elapsed: Duration, mut report: impl FnMut(bool, &[f64])) {
        self.pending.retain_mut(|(visible, frames)| {
            frames.push(elapsed.as_secs_f64() * 1000.0);
            if frames.len() == 3 {
                report(*visible, frames);
                false
            } else {
                true
            }
        });
    }
}

impl PerfLog {
    fn suspend(&mut self) {
        *self = Self::default();
    }

    fn record(
        &mut self,
        now: Instant,
        frame: Duration,
        cpu: Duration,
        gpu: &[f32],
        frame_id: Option<u64>,
        samples: &[(u64, f32)],
        raw: &[Span],
    ) -> Option<Stats> {
        if frame > Duration::from_secs(2) {
            self.suspend();
            self.start = Some(now);
            return None;
        }
        // Ignore the first interval: it may include loading or time spent in menus.
        let Some(start) = self.start else {
            self.start = Some(now);
            return None;
        };
        let ms = frame.as_secs_f64() * 1000.0;
        self.frames.push(ms);
        self.slow_frames += usize::from(frame > Duration::from_millis(33));
        if ms > self.worst.ms {
            let mut ranked = spans::merge(raw);
            // Only outer spans cover new time; nested spans must not hide unspanned gaps.
            let covered = raw
                .iter()
                .filter(|s| s.depth == 0)
                .map(|s| s.ms)
                .sum::<f32>();
            ranked.push(Span {
                name: "unspanned",
                ms: (ms as f32 - covered).max(0.0),
                cpu_ms: 0.0,
                depth: 0,
            });
            ranked.sort_by(|a, b| b.ms.total_cmp(&a.ms));
            ranked.retain(|span| span.ms > 0.0);
            ranked.truncate(5);
            self.worst = WorstFrame {
                ms,
                frame_id,
                gpu_ms: None,
                spans: ranked,
            };
        }
        // Readbacks are asynchronous: use only this frame's ID, never the latest GPU sample.
        for &(id, ms) in samples {
            if self.worst.frame_id == Some(id) {
                self.worst.gpu_ms = Some(ms);
            }
        }
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
            slow_frames: self.slow_frames,
            worst: std::mem::take(&mut self.worst),
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
        self.slow_frames = 0;
        Some(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(log: &mut PerfLog, now: Instant, ms: u64) -> Option<Stats> {
        log.record(
            now,
            Duration::from_millis(ms),
            Duration::from_millis(3),
            &[9.0],
            None,
            &[],
            &[],
        )
    }

    #[test]
    fn resume_discards_first_frame_even_after_background_redraws() {
        let mut log = PerfLog::default();
        let now = Instant::now();
        record(&mut log, now, 16);
        record(&mut log, now + Duration::from_secs(1), 80);
        log.suspend();
        record(&mut log, now + Duration::from_secs(20), 16);
        // Resuming resets again, even if a redraw occurred while backgrounded.
        log.suspend();
        assert!(record(&mut log, now + Duration::from_secs(38), 38_000).is_none());
        let stats = record(&mut log, now + Duration::from_secs(48), 16).unwrap();
        assert_eq!(stats.fps, 62.5);
        assert_eq!(stats.low_1, 16.0);
        assert_eq!(stats.low_01, 16.0);
        assert_eq!(stats.cpu_ms, 3.0);
        assert_eq!(stats.gpu_ms, Some(9.0));
        assert_eq!(stats.slow_frames, 0);
        assert_eq!(stats.worst.ms, 16.0);
        assert_eq!(stats.worst.describe(), "unspanned 16.0");
        // A brief resume gap must also be discarded, independently of the 2 s guard.
        log.suspend();
        record(&mut log, now + Duration::from_secs(49), 100);
        let stats = record(&mut log, now + Duration::from_secs(59), 16).unwrap();
        assert_eq!(stats.low_1, 16.0);
    }

    #[test]
    fn huge_frame_restarts_interval_without_polluting_statistics() {
        let mut log = PerfLog::default();
        let now = Instant::now();
        record(&mut log, now, 16);
        record(&mut log, now + Duration::from_secs(1), 80);
        assert!(record(&mut log, now + Duration::from_secs(11), 2001).is_none());
        let stats = record(&mut log, now + Duration::from_secs(21), 16).unwrap();
        assert_eq!(stats.fps, 62.5);
        assert_eq!(stats.low_1, 16.0);
        assert_eq!(stats.slow_frames, 0);
    }

    #[test]
    fn worst_frame_keeps_ranked_spans_and_its_delayed_gpu_sample() {
        let now = Instant::now();
        let mut log = PerfLog::default();
        record(&mut log, now, 16);
        let raw = [
            Span {
                name: "upload_chunks",
                ms: 40.0,
                cpu_ms: 3.0,
                depth: 1,
            },
            Span {
                name: "update_world",
                ms: 50.0,
                cpu_ms: 8.0,
                depth: 0,
            },
            Span {
                name: "mods_frame",
                ms: 10.0,
                cpu_ms: 9.0,
                depth: 0,
            },
        ];
        log.record(
            now + Duration::from_secs(1),
            Duration::from_millis(100),
            Duration::from_millis(20),
            &[],
            Some(7),
            &[(6, 99.0)],
            &raw,
        );
        log.record(
            now + Duration::from_secs(2),
            Duration::from_millis(34),
            Duration::from_millis(2),
            &[],
            Some(8),
            &[(7, 12.0)],
            &[],
        );
        let stats = record(&mut log, now + Duration::from_secs(10), 33).unwrap();
        assert_eq!(stats.slow_frames, 2);
        assert_eq!(stats.worst.ms, 100.0);
        assert_eq!(stats.worst.gpu_ms, Some(12.0));
        assert_eq!(
            stats.worst.describe(),
            "update_world 50.0/8.0, upload_chunks 40.0/3.0, unspanned 40.0, mods_frame 10.0/9.0"
        );
        let next = record(&mut log, now + Duration::from_secs(20), 16).unwrap();
        assert_eq!(next.slow_frames, 0);
        assert_eq!(next.worst.gpu_ms, None);
        assert_eq!(next.worst.describe(), "unspanned 16.0");
    }

    #[test]
    fn worst_breakdown_keeps_only_the_five_largest_costs() {
        let now = Instant::now();
        let mut log = PerfLog::default();
        record(&mut log, now, 16);
        let raw: Vec<_> = ["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| Span {
                name,
                ms: (i + 1) as f32,
                cpu_ms: 0.5,
                depth: 0,
            })
            .collect();
        log.record(
            now + Duration::from_secs(1),
            Duration::from_millis(40),
            Duration::ZERO,
            &[],
            None,
            &[],
            &raw,
        );
        let stats = record(&mut log, now + Duration::from_secs(10), 16).unwrap();
        assert_eq!(
            stats.worst.spans.iter().map(|s| s.name).collect::<Vec<_>>(),
            ["unspanned", "f", "e", "d", "c"]
        );
        assert_eq!(stats.worst.spans[0].ms, 19.0);
    }

    #[test]
    fn keyboard_reports_only_transitions_and_the_next_three_frames() {
        let mut frames = KeyboardFrames::default();
        let mut reports = Vec::new();
        frames.visibility(true);
        frames.visibility(true); // Animation insets are not another show.
        frames.frame(Duration::from_millis(80), |v, ms| {
            reports.push((v, ms.to_vec()))
        });
        frames.visibility(false);
        for ms in [30, 20, 16, 16] {
            frames.frame(Duration::from_millis(ms), |v, ms| {
                reports.push((v, ms.to_vec()))
            });
        }
        assert_eq!(
            reports,
            vec![
                (true, vec![80.0, 30.0, 20.0]),
                (false, vec![30.0, 20.0, 16.0])
            ]
        );
        frames.visibility(true);
        frames.clear();
        for _ in 0..3 {
            frames.frame(Duration::from_secs(38), |_, _| {
                panic!("background gap reported")
            });
        }
    }

    #[test]
    fn usage_fields_list_only_what_the_os_reported() {
        use horizon_threads::process_usage::Rates;
        assert_eq!(usage_fields(None), "");
        let mut r = Rates {
            cpu_ms_per_s: 250.0,
            context_switches_per_s: 500.0,
            energy_mw: None,
            idle_wakeups_per_s: None,
            interrupt_wakeups_per_s: None,
            gpu_ms_per_s: None,
        };
        assert_eq!(
            usage_fields(Some(&r)),
            " proc_cpu_ms_s=250 ctx_switches_s=500"
        );
        r.idle_wakeups_per_s = Some(40.0);
        r.interrupt_wakeups_per_s = Some(12.0);
        r.gpu_ms_per_s = Some(300.0);
        let text = usage_fields(Some(&r));
        assert!(
            text.ends_with(" idle_wakeups_s=40 intr_wakeups_s=12 gpu_ms_s=300"),
            "{text}"
        );
        assert!(!text.contains("energy_mw"));
        r.energy_mw = Some(1000.0);
        assert!(usage_fields(Some(&r)).ends_with(" energy_mw=1000"));
    }

    #[test]
    fn screens_cap_the_phone_frame_rate() {
        use PhoneScreen::*;
        for (limit, screen, expected) in [
            (120, Gameplay, 120),
            (120, PausedSingleplayer, 30),
            (20, PausedSingleplayer, 20),
            (120, InWorldScreen, 60),
            (120, OutOfWorld, 60),
            (30, OutOfWorld, 30),
        ] {
            assert_eq!(
                phone_screen_cap(limit, screen),
                expected,
                "{limit} {screen:?}"
            );
        }
    }
}
