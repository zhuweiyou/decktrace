//! 通用运行时：采集流 -> 识别 -> 记牌状态机。
//!
//! 不绑定 UI：GUI 应用与测试复用 `Session`。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use capture::{CaptureStream, Platform, WindowInfo};
use tracking::{CardTracker, Detection, GameProfile, SharedRecognizer, TrackerSnapshot};

struct AutoReset {
    enabled: bool,
    prev_hand_ids: BTreeSet<String>,
    prev_hand_count: usize,
    last_hand_seen: f64,
    last_reset: f64,
    resets: usize,
}

impl AutoReset {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            prev_hand_ids: BTreeSet::new(),
            prev_hand_count: 0,
            last_hand_seen: 0.0,
            last_reset: 0.0,
            resets: 0,
        }
    }

    fn reset_baseline(&mut self) {
        self.prev_hand_ids.clear();
        self.prev_hand_count = 0;
        self.last_hand_seen = 0.0;
        self.last_reset = 0.0;
    }

    fn maybe_reset(
        &mut self,
        tracker: &mut CardTracker,
        game: &GameProfile,
        detections: &[Detection],
        height: u32,
        now: f64,
    ) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let cfg = &game.reset;
        let hand_ids: BTreeSet<String> = detections
            .iter()
            .filter(|det| {
                det.bbox.1 as f32 > height as f32 * cfg.hand_top_ratio
                    && (det.bbox.1 + det.bbox.3) as f32 > height as f32 * cfg.hand_bottom_ratio
            })
            .map(|det| det.card_id())
            .collect();

        if hand_ids.len() < cfg.min_hand_cards {
            if now - self.last_hand_seen > cfg.absent_seconds {
                self.prev_hand_count = 0;
            }
            return None;
        }

        let seen = tracker.seen_count();
        let jump = self.prev_hand_count <= cfg.empty_hand_threshold
            && hand_ids.len() >= cfg.min_hand_for_jump
            && seen > 0;
        let overlap = if self.prev_hand_ids.is_empty() {
            0.0
        } else {
            hand_ids.intersection(&self.prev_hand_ids).count() as f64 / hand_ids.len() as f64
        };
        let changed = !self.prev_hand_ids.is_empty()
            && seen >= cfg.seen_for_reset
            && now - self.last_reset > cfg.min_reset_gap
            && overlap < cfg.overlap_threshold as f64;

        if jump || changed {
            let reason = if jump {
                "手牌张数跳变".to_string()
            } else {
                format!("手牌重合度 {:.0}%", overlap * 100.0)
            };
            let before = seen;
            tracker.reset();
            self.resets += 1;
            self.last_reset = now;
            self.prev_hand_ids = hand_ids;
            self.prev_hand_count = self.prev_hand_ids.len();
            self.last_hand_seen = now;
            eprintln!("\n[重置] {reason}，清空前已识别 {before} 张");
            return Some(reason);
        }

        self.prev_hand_ids = hand_ids;
        self.prev_hand_count = self.prev_hand_ids.len();
        self.last_hand_seen = now;
        None
    }

    fn manual_reset(&mut self, tracker: &mut CardTracker) {
        tracker.reset();
        self.reset_baseline();
        self.resets += 1;
        eprintln!("\n[重置] 手动");
    }
}

pub struct SessionConfig {
    pub fps: f32,
    pub min_hits: usize,
    pub instant_score: f32,
    pub pending_ttl: f64,
    pub auto_reset: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            fps: 15.0,
            min_hits: 2,
            instant_score: 0.92,
            pending_ttl: 1.6,
            auto_reset: true,
        }
    }
}

/// 一局实时会话：采集、识别、底牌门控、自动重置。
pub struct Session {
    game: Arc<GameProfile>,
    recognizer: SharedRecognizer,
    tracker: CardTracker,
    auto_reset: AutoReset,
    stream: Box<dyn CaptureStream>,
    started: Instant,
    frame_no: u64,
    trace: bool,
    trace_seen: usize,
    trace_armed: bool,
    trace_bottoms: BTreeSet<String>,
    trace_bottom_det: BTreeSet<String>,
}

impl Session {
    pub fn new(
        platform: &dyn Platform,
        game: GameProfile,
        window: &WindowInfo,
        recognizer: SharedRecognizer,
        config: SessionConfig,
    ) -> Result<Self> {
        let stream = platform.create_capture_stream(window, config.fps)?;
        let game = Arc::new(game);
        let tracker = CardTracker::new(
            game.clone(),
            config.min_hits,
            config.instant_score,
            config.pending_ttl,
        );
        Ok(Self {
            game,
            recognizer,
            tracker,
            auto_reset: AutoReset::new(config.auto_reset),
            stream,
            started: Instant::now(),
            frame_no: 0,
            trace: std::env::var("DECKTRACE_TRACE").is_ok(),
            trace_seen: usize::MAX,
            trace_armed: false,
            trace_bottoms: BTreeSet::new(),
            trace_bottom_det: BTreeSet::new(),
        })
    }

    pub fn start(&mut self) -> Result<()> {
        self.stream.start()?;
        self.started = Instant::now();
        Ok(())
    }

    pub fn tick(&mut self, timeout: Duration) -> Result<Option<TrackerSnapshot>> {
        let frame = match self.stream.read(timeout) {
            Ok(frame) => frame,
            Err(err) => {
                return Err(err.into());
            }
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        self.frame_no += 1;
        let now = self.started.elapsed().as_secs_f64();
        let detect_jokers = self.frame_no.is_multiple_of(2) && !self.tracker.jokers_known();
        let result = self.recognizer.recognize(&frame.image, detect_jokers);
        self.auto_reset.maybe_reset(
            &mut self.tracker,
            self.game.as_ref(),
            &result.detections,
            frame.height,
            now,
        );
        let committed = self.tracker.observe(&result.detections, now);
        let snapshot = self.tracker.snapshot();
        if self.trace {
            let bottom_det: BTreeSet<String> = result
                .detections
                .iter()
                .filter(|det| det.is_bottom)
                .map(|det| det.card_id())
                .collect();
            let changed = !committed.is_empty()
                || snapshot.armed != self.trace_armed
                || snapshot.seen_count != self.trace_seen
                || snapshot.bottom_ids != self.trace_bottoms;
            if changed || (!bottom_det.is_empty() && bottom_det != self.trace_bottom_det) {
                let new_ids: Vec<String> =
                    committed.iter().map(|det| det.card_id()).collect();
                eprintln!(
                    "[trace] t={now:.1}s armed={} seen={} remaining={} new={new_ids:?} bottoms={:?} bottom_det={bottom_det:?} det={} {:.1}ms",
                    snapshot.armed,
                    snapshot.seen_count,
                    snapshot.remaining,
                    snapshot.bottom_ids,
                    result.detections.len(),
                    result.elapsed_ms
                );
                self.trace_seen = snapshot.seen_count;
                self.trace_armed = snapshot.armed;
                self.trace_bottoms = snapshot.bottom_ids.clone();
                self.trace_bottom_det = bottom_det;
            }
        }
        Ok(Some(snapshot))
    }

    pub fn reset(&mut self) {
        self.auto_reset.manual_reset(&mut self.tracker);
    }

    pub fn snapshot(&self) -> TrackerSnapshot {
        self.tracker.snapshot()
    }

    pub fn stop(&mut self) {
        self.stream.stop();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stream.stop();
    }
}
