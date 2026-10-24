//! 通用运行时：采集流 -> 识别 -> 记牌状态机。
//!
//! 这里不绑定 UI：TUI 和独立 GUI 应用都复用 `Session`。

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use decktrace_capture::{CaptureStream, Platform};
use decktrace_core::{CardTracker, Detection, TrackerSnapshot, WindowInfo};
use decktrace_game_api::GameProfile;
use decktrace_recognition::{CardRecognizer, annotate};

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
    game: GameProfile,
    recognizer: CardRecognizer,
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
    ) -> Result<Self> {
        let recognizer = CardRecognizer::new(&game).map_err(|err| anyhow::anyhow!("{err}"))?;
        let stream = platform.create_capture_stream(window, fps)?;
        let tracker = CardTracker::new(game.deck_rules(), min_hits, 0.92, 1.6);
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
        let result = self
            .recognizer
            .recognize(&frame.image, self.frame_no % 2 == 0);
        self.last_recognition_ms = result.elapsed_ms;
        self.auto_reset.maybe_reset(
            &mut self.tracker,
            &self.game,
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
        &self.game
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

pub fn run_tui(
    platform: &dyn Platform,
    game: GameProfile,
    window: &WindowInfo,
    mut renderer: Box<dyn decktrace_ui::Renderer>,
    options: RunOptions,
) -> Result<()> {
    std::fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("创建输出目录失败：{}", options.out_dir.display()))?;
    let mut session = Session::new(
        platform,
        game,
        window,
        15.0,
        options.min_hits,
        options.auto_reset,
    )?;
    renderer.start().map_err(|err| anyhow::anyhow!(err))?;
    if let Err(err) = session.start() {
        renderer.close();
        return Err(err);
    }
    let started = Instant::now();
    let mut last_state_write = 0.0;
    let mut last_debug_save = 0.0;
    let interactive = std::io::stdin().is_terminal();

    let result = (|| -> Result<()> {
        loop {
            let now = started.elapsed().as_secs_f64();
            if options.duration.is_some_and(|duration| now >= duration) {
                break;
            }
            if interactive && handle_keys(&mut session) {
                break;
            }
            if !renderer.is_open() {
                break;
            }
            let Some(tick) = session.tick(Duration::from_millis(200))? else {
                continue;
            };
            if now - last_state_write >= 0.2 {
                let _ = write_state(&options.out_dir, &tick.snapshot, &tick.detections);
                last_state_write = now;
            }
            if options.debug && now - last_debug_save >= 0.5 {
                let annotated = annotate(&tick.image, &tick.detections);
                let _ = annotated.save(options.out_dir.join("annotated.png"));
                last_debug_save = now;
            }
            renderer.update(&tick.snapshot, session.game(), &session.status());
        }
        Ok(())
    })();

    session.stop();
    renderer.close();
    let snapshot = session.snapshot();
    println!(
        "已停止：识别 {} 张，剩余 {} 张，自动重置 {} 次。",
        snapshot.seen_count,
        snapshot.remaining,
        session.auto_reset_count()
    );
    result
}

fn handle_keys(session: &mut Session) -> bool {
    use crossterm::event::{Event, KeyCode, KeyModifiers, poll, read};
    while poll(Duration::ZERO).unwrap_or(false) {
        if let Ok(Event::Key(key)) = read() {
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q') => return true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return true;
                }
                KeyCode::Char('r') | KeyCode::Char('R') => session.reset(),
                _ => {}
            }
        }
    }
    false
}

pub struct RunOptions {
    pub auto_reset: bool,
    pub min_hits: usize,
    pub debug: bool,
    pub duration: Option<f64>,
    pub out_dir: std::path::PathBuf,
}

pub struct OnceResult {
    pub snapshot: TrackerSnapshot,
    pub recognition_ms: f64,
    pub detections: Vec<Detection>,
    pub annotated: image::RgbImage,
}

pub fn run_once_on_image(game: &GameProfile, path: &Path, debug: bool) -> Result<OnceResult> {
    let image = image::open(path)?.to_rgb8();
    let recognizer = CardRecognizer::new(game).map_err(|err| anyhow::anyhow!("{err}"))?;
    let result = recognizer.recognize(&image, true);
    let mut tracker = CardTracker::new(game.deck_rules(), 1, 0.92, 1.6);
    // 单帧没有后续帧，喂两遍以便确认底牌后开局。
    tracker.observe(&result.detections, 0.0);
    tracker.observe(&result.detections, 0.1);
    if debug {
        std::fs::create_dir_all("frames").ok();
        let _ = annotate(&image, &result.detections).save("frames/annotated.png");
    }
    let annotated = annotate(&image, &result.detections);
    Ok(OnceResult {
        snapshot: tracker.snapshot(),
        recognition_ms: result.elapsed_ms,
        detections: result.detections,
        annotated,
    })
}

pub fn run_once_live(
    platform: &dyn Platform,
    game: &GameProfile,
    window: &WindowInfo,
    debug: bool,
) -> Result<OnceResult> {
    let mut stream = platform.create_capture_stream(window, 15.0)?;
    stream.start()?;
    let frame = stream
        .read(Duration::from_secs(5))?
        .ok_or_else(|| anyhow::anyhow!("5 秒内没有采集到帧"))?;
    stream.stop();
    let recognizer = CardRecognizer::new(game).map_err(|err| anyhow::anyhow!("{err}"))?;
    let result = recognizer.recognize(&frame.image, true);
    let mut tracker = CardTracker::new(game.deck_rules(), 1, 0.92, 1.6);
    tracker.observe(&result.detections, 0.0);
    tracker.observe(&result.detections, 0.1);
    if debug {
        std::fs::create_dir_all("frames").ok();
        let _ = annotate(&frame.image, &result.detections).save("frames/annotated.png");
    }
    let annotated = annotate(&frame.image, &result.detections);
    Ok(OnceResult {
        snapshot: tracker.snapshot(),
        recognition_ms: result.elapsed_ms,
        detections: result.detections,
        annotated,
    })
}

pub fn capture_one(platform: &dyn Platform, window: &WindowInfo, out_path: &Path) -> Result<()> {
    let mut stream = platform.create_capture_stream(window, 15.0)?;
    stream.start()?;
    let frame = stream
        .read(Duration::from_secs(5))?
        .ok_or_else(|| anyhow::anyhow!("5 秒内没有采集到帧"))?;
    stream.stop();
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    frame.image.save(out_path)?;
    Ok(())
}

fn write_state(out_dir: &Path, snapshot: &TrackerSnapshot, detections: &[Detection]) -> Result<()> {
    let path = out_dir.join("state.json");
    let tmp = out_dir.join("state.tmp.json");
    let payload = serde_json::json!({
        "armed": snapshot.armed,
        "seen_count": snapshot.seen_count,
        "remaining": snapshot.remaining,
        "counts": snapshot.counts,
        "seen_ids": snapshot.seen_ids,
        "bottom_ids": snapshot.bottom_ids,
        "recent": snapshot.recent.iter().map(|d| d.label()).collect::<Vec<_>>(),
        "detections": detections.iter().map(|d| serde_json::json!({
            "id": d.card_id(),
            "label": d.label(),
            "score": (d.score * 1000.0).round() / 1000.0,
            "box": [d.bbox.0, d.bbox.1, d.bbox.2, d.bbox.3],
            "bottom": d.is_bottom,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&tmp, serde_json::to_vec_pretty(&payload)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn timestamp_slug() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{millis}")
}
