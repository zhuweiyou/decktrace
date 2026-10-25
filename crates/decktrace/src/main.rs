use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossbeam_channel::{Receiver, Sender, bounded};
use decktrace::{Session, SessionConfig, create_recognizer, get_game, list_games};
use decktrace_capture::{Platform, WindowInfo, select_window};
use decktrace_tracking::{CardTracker, GameProfile, TrackerSnapshot, WindowSelector};
use eframe::egui;
use image::{Rgba, RgbaImage};

mod window_state;

const CARD_W: u32 = 15;
const CARD_H: u32 = 30;
const MARGIN: u32 = 4;

#[derive(Parser, Debug)]
#[command(name = "decktrace", about = "斗地主记牌器", version)]
struct Args {
    #[arg(long, default_value = "auto", help = "游戏 ID；auto 自动检测")]
    game: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    owner: Option<String>,
    #[arg(long)]
    window_id: Option<u32>,
    #[arg(long, default_value_t = 2)]
    min_hits: usize,
    #[arg(long, default_value_t = 15.0)]
    fps: f32,
    #[arg(long)]
    no_auto_reset: bool,
}

enum Message {
    Snapshot(TrackerSnapshot),
    Error(String),
}

fn main() -> Result<()> {
    let args = Args::parse();
    let (tx, rx) = bounded::<Message>(1);
    let stop = Arc::new(AtomicBool::new(false));
    let reset = Arc::new(AtomicBool::new(false));
    let game_slot = Arc::new(OnceLock::new());
    spawn_worker(
        args,
        tx,
        rx.clone(),
        stop.clone(),
        reset.clone(),
        game_slot.clone(),
    )?;

    let app = DeckApp::new(rx, stop, reset, game_slot);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("斗地主记牌器")
        .with_inner_size([
            (CARD_W * 54 + MARGIN * 2) as f32,
            (CARD_H + MARGIN * 2) as f32,
        ])
        .with_resizable(false)
        .with_always_on_top();
    if let Some(pos) = window_state::load() {
        viewport = viewport.with_position(pos);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native("斗地主记牌器", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|err| anyhow::anyhow!("GUI 启动失败：{err}"))?;
    Ok(())
}

fn window_selector(args: &Args, game: &GameProfile) -> WindowSelector {
    let mut selector = game.window.clone();
    if let Some(title) = &args.title {
        selector.title = title.clone();
    }
    if let Some(owner) = &args.owner {
        selector.app = Some(owner.clone());
    }
    if let Some(id) = args.window_id {
        selector.window_id = Some(id);
    }
    selector
}

/// 自动检测当前打开的游戏窗口。仅有一个匹配时直接使用；
/// 多个匹配时按注册顺序取第一个；没有匹配时返回 None，由 worker 重试。
fn detect_game(platform: &dyn Platform, args: &Args) -> Result<Option<(GameProfile, WindowInfo)>> {
    let windows = platform.window_service().list_windows(false)?;

    if args.game != "auto" {
        let game = get_game(&args.game).map_err(anyhow::Error::msg)?;
        let selector = window_selector(args, &game);
        return Ok(select_window(&windows, &selector)
            .ok()
            .map(|window| (game, window)));
    }

    let mut found = Vec::new();
    for game in list_games() {
        if game.validate().is_err() {
            continue;
        }
        let selector = window_selector(args, &game);
        if let Ok(window) = select_window(&windows, &selector) {
            found.push((game, window));
        }
    }
    if found.len() > 1 {
        eprintln!(
            "检测到 {} 个游戏窗口，使用 {}；可用 --game 指定",
            found.len(),
            found[0].0.name
        );
    }
    Ok(found.into_iter().next())
}

fn spawn_worker(
    args: Args,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    game_slot: Arc<OnceLock<GameProfile>>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let platform = create_platform()?;
                let (game, window) = loop {
                    if stop.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    match detect_game(platform.as_ref(), &args) {
                        Ok(Some(found)) => break found,
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(500));
                        }
                        Err(err) => {
                            let _ = tx.send(Message::Error(err.to_string()));
                            return Err(err);
                        }
                    }
                };
                let recognizer = create_recognizer(&game).map_err(anyhow::Error::msg)?;
                game_slot.set(game.clone()).ok();
                let mut session = Session::new(
                    platform.as_ref(),
                    game,
                    &window,
                    recognizer,
                    SessionConfig {
                        fps: args.fps,
                        min_hits: args.min_hits,
                        auto_reset: !args.no_auto_reset,
                        ..SessionConfig::default()
                    },
                )?;
                session.start()?;
                while !stop.load(Ordering::Relaxed) {
                    if reset.swap(false, Ordering::Relaxed) {
                        session.reset();
                    }
                    match session.tick(Duration::from_millis(200)) {
                        Ok(Some(snapshot)) => {
                            if tx.try_send(Message::Snapshot(snapshot)).is_err() {
                                let _ = rx.try_recv();
                                let _ = tx.try_send(Message::Snapshot(session.snapshot()));
                            }
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let _ = tx.send(Message::Error(err.to_string()));
                            break;
                        }
                    }
                }
                session.stop();
                Ok(())
            })();
            if let Err(err) = result {
                eprintln!("GUI 采集失败：{err}");
                let _ = tx.send(Message::Error(err.to_string()));
            }
        })
        .map_err(|err| anyhow::anyhow!("启动采集线程失败：{err}"))?;
    Ok(())
}

struct DeckApp {
    game: Option<GameProfile>,
    game_slot: Arc<OnceLock<GameProfile>>,
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    snapshot: Option<TrackerSnapshot>,
    error: Option<String>,
    sources: HashMap<String, RgbaImage>,
    textures: HashMap<(String, bool), egui::TextureHandle>,
    badge: Option<egui::TextureHandle>,
    last_key: Option<(Vec<String>, BTreeSet<String>)>,
    last_pos: Option<egui::Pos2>,
}

impl DeckApp {
    fn new(
        rx: Receiver<Message>,
        stop: Arc<AtomicBool>,
        reset: Arc<AtomicBool>,
        game_slot: Arc<OnceLock<GameProfile>>,
    ) -> Self {
        Self {
            game: None,
            game_slot,
            rx,
            stop,
            reset,
            snapshot: None,
            error: None,
            sources: HashMap::new(),
            textures: HashMap::new(),
            badge: None,
            last_key: None,
            last_pos: None,
        }
    }

    fn activate_game(&mut self, game: GameProfile) {
        let tracker = CardTracker::new(Arc::new(game.clone()), 2, 0.92, 1.6);
        self.snapshot = Some(tracker.snapshot());
        self.game = Some(game);
        self.sources.clear();
        self.textures.clear();
        self.last_key = None;
    }

    fn drain(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Snapshot(snapshot) => self.snapshot = Some(snapshot),
                Message::Error(error) => self.error = Some(error),
            }
        }
    }

    fn ensure_textures(&mut self, ctx: &egui::Context, cards: &[(String, bool)]) {
        for (card_id, inverted) in cards {
            let key = (card_id.clone(), *inverted);
            if self.textures.contains_key(&key) {
                continue;
            }
            let Some(source) = self.source(card_id) else {
                continue;
            };
            let image = if *inverted {
                invert(source)
            } else {
                source.clone()
            };
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("{card_id}-{}", if *inverted { "inv" } else { "normal" }),
                color,
                egui::TextureOptions::NEAREST,
            );
            self.textures.insert(key, texture);
        }
    }

    fn source(&mut self, card_id: &str) -> Option<&RgbaImage> {
        if !self.sources.contains_key(card_id) {
            let path = self
                .game
                .as_ref()?
                .card_assets_dir
                .join(format!("{card_id}.png"));
            let image = image::open(path).ok()?.to_rgba8();
            self.sources.insert(card_id.to_string(), image);
        }
        self.sources.get(card_id)
    }
}

impl eframe::App for DeckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        if self.game.is_none()
            && let Some(game) = self.game_slot.get()
        {
            self.activate_game(game.clone());
        }
        let ctx = ui.ctx().clone();
        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            self.last_pos = Some(rect.min);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::R)) {
            self.reset.store(true, Ordering::Relaxed);
        }

        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(241, 236, 225));
        if self.error.is_some() {
            // egui 默认字体没有中文，错误详情已经在 worker 里输出到 stderr。
            ui.label("Screen capture failed - check Screen Recording permission.");
            return;
        }
        let Some(game) = self.game.as_ref() else {
            ui.label("Waiting for game window...");
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            ui.label("Waiting for game window...");
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        };

        let remaining = game.remaining_card_ids(&snapshot.seen_ids);
        let bombs = bomb_card_ids(&remaining, game);
        let cards: Vec<(String, bool)> = remaining
            .iter()
            .map(|card_id| (card_id.clone(), bombs.contains(card_id)))
            .collect();
        let bottom_ids = snapshot.bottom_ids.clone();

        let key = (remaining.clone(), bombs.clone());
        if self.last_key.as_ref() != Some(&key) {
            self.last_key = Some(key);
            self.ensure_textures(&ctx, &cards);
        }
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                (CARD_W * 54 + MARGIN * 2) as f32,
                (CARD_H + MARGIN * 2) as f32,
            ),
            egui::Sense::hover(),
        );
        if self.badge.is_none() {
            self.badge = load_bottom_badge(&ctx);
        }
        for (index, (card_id, inverted)) in cards.iter().enumerate() {
            let Some(texture) = self.textures.get(&(card_id.clone(), *inverted)) else {
                continue;
            };
            let pos =
                rect.min + egui::vec2(MARGIN as f32 + index as f32 * CARD_W as f32, MARGIN as f32);
            let card_rect =
                egui::Rect::from_min_size(pos, egui::vec2(CARD_W as f32, CARD_H as f32));
            ui.painter().image(
                texture.id(),
                card_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if bottom_ids.contains(card_id)
                && let Some(badge) = &self.badge
            {
                let badge_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        card_rect.min.x + (CARD_W as f32 - 9.0) / 2.0,
                        card_rect.max.y - 9.0 - 2.0,
                    ),
                    egui::vec2(9.0, 9.0),
                );
                ui.painter().image(
                    badge.id(),
                    badge_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        ctx.request_repaint_after(Duration::from_millis(80));
    }
}

impl Drop for DeckApp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(pos) = self.last_pos {
            window_state::save(pos);
        }
    }
}

fn load_bottom_badge(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    const BADGE: &[u8] = include_bytes!("../assets/bottom_badge.png");
    let image = image::load_from_memory(BADGE).ok()?.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    Some(ctx.load_texture("bottom-badge", color, egui::TextureOptions::LINEAR))
}

fn invert(source: &RgbaImage) -> RgbaImage {
    let mut out = source.clone();
    for pixel in out.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        *pixel = Rgba([255 - r, 255 - g, 255 - b, a]);
    }
    out
}

fn bomb_card_ids(remaining: &[String], game: &GameProfile) -> BTreeSet<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for card_id in remaining {
        *counts.entry(rank_of(card_id, game)).or_insert(0) += 1;
    }
    let mut result = BTreeSet::new();
    for (rank, count) in &counts {
        let total = game.rank_totals.get(rank).copied().unwrap_or(0) as usize;
        if total >= 4 && *count >= 4 {
            result.extend(
                remaining
                    .iter()
                    .filter(|card_id| rank_of(card_id, game) == *rank)
                    .cloned(),
            );
        }
    }
    let has_bj = remaining.iter().any(|card| card == "BJ");
    let has_sj = remaining.iter().any(|card| card == "SJ");
    if has_bj && has_sj {
        result.insert("BJ".to_string());
        result.insert("SJ".to_string());
    }
    result
}

fn rank_of(card_id: &str, game: &GameProfile) -> String {
    if game.labels.contains_key(card_id) {
        return card_id.to_string();
    }
    card_id[..card_id.len() - 1].to_string()
}

fn create_platform() -> Result<Box<dyn Platform>> {
    Ok(decktrace_capture::create_platform()?)
}
