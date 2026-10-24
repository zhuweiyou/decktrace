//! 通用运行时：采集流 -> 识别 -> 记牌状态机。
//!
//! 不绑定 UI：GUI 应用与测试复用 `Session`。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use decktrace_capture::{CaptureStream, Platform, WindowInfo};
use decktrace_tracking::{CardTracker, Detection, GameProfile, SharedRecognizer, TrackerSnapshot};

pub struct Tick {
    pub snapshot: TrackerSnapshot,
    pub detections: Vec<Detection>,
    pub image: image::RgbImage,
    pub recognition_ms: f64,
}

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

        let seen = tracker.snapshot().seen_count;
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

/// 一局实时会话：采集、识别、底牌门控、自动重置。
pub struct Session {
    game: Arc<GameProfile>,
    recognizer: SharedRecognizer,
    tracker: CardTracker,
    auto_reset: AutoReset,
    stream: Box<dyn CaptureStream>,
    started: Instant,
    frames: u64,
    frame_no: u64,
    errors: u64,
    last_recognition_ms: f64,
}

impl Session {
    pub fn new(
        platform: &dyn Platform,
        game: GameProfile,
        window: &WindowInfo,
        fps: f32,
        min_hits: usize,
        auto_reset: bool,
        recognizer: SharedRecognizer,
    ) -> Result<Self> {
        let stream = platform.create_capture_stream(window, fps)?;
        let game = Arc::new(game);
        let tracker = CardTracker::new(game.clone(), min_hits, 0.92, 1.6);
        Ok(Self {
            game,
            recognizer,
            tracker,
            auto_reset: AutoReset::new(auto_reset),
            stream,
            started: Instant::now(),
            frames: 0,
            frame_no: 0,
            errors: 0,
            last_recognition_ms: 0.0,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        self.stream.start()?;
        self.started = Instant::now();
        Ok(())
    }

    pub fn tick(&mut self, timeout: Duration) -> Result<Option<Tick>> {
        let frame = match self.stream.read(timeout) {
            Ok(frame) => frame,
            Err(err) => {
                self.errors += 1;
                return Err(err.into());
            }
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        self.frames += 1;
        self.frame_no += 1;
        let now = self.started.elapsed().as_secs_f64();
        let detect_jokers = self.frame_no % 2 == 0 && !self.tracker.jokers_known();
        let result = self.recognizer.recognize(&frame.image, detect_jokers);
        self.last_recognition_ms = result.elapsed_ms;
        self.auto_reset.maybe_reset(
            &mut self.tracker,
            self.game.as_ref(),
            &result.detections,
            frame.height,
            now,
        );
        self.tracker.observe(&result.detections, now);
        Ok(Some(Tick {
            snapshot: self.tracker.snapshot(),
            detections: result.detections,
            image: frame.image,
            recognition_ms: result.elapsed_ms,
        }))
    }

    pub fn reset(&mut self) {
        self.auto_reset.manual_reset(&mut self.tracker);
    }

    pub fn snapshot(&self) -> TrackerSnapshot {
        self.tracker.snapshot()
    }

    pub fn game(&self) -> &GameProfile {
        self.game.as_ref()
    }

    pub fn auto_reset_count(&self) -> usize {
        self.auto_reset.resets
    }

    pub fn errors(&self) -> u64 {
        self.errors
    }

    pub fn recognition_ms(&self) -> f64 {
        self.last_recognition_ms
    }

    pub fn status(&self) -> String {
        let elapsed = self.started.elapsed().as_secs_f64().max(1e-6);
        format!(
            "{:.1}fps · 识别 {:.0}ms · 错 {} · 重置 {} · {}",
            self.frames as f64 / elapsed,
            self.last_recognition_ms,
            self.errors,
            self.auto_reset.resets,
            self.stream.backend_name()
        )
    }

    pub fn backend_name(&self) -> &str {
        self.stream.backend_name()
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
