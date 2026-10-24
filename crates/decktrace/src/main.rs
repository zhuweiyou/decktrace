use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossbeam_channel::{Receiver, Sender, bounded};
use decktrace::{Session, SessionConfig, get_game};
use decktrace_capture::Platform;
use decktrace_jj::JjRecognizer;
use decktrace_tracking::{
    CardTracker, GameProfile, SharedRecognizer, TrackerSnapshot, WindowSelector,
};
use eframe::egui;
use image::{Rgba, RgbaImage};

const CARD_W: u32 = 15;
const CARD_H: u32 = 30;
const MARGIN: u32 = 4;

#[derive(Parser, Debug)]
#[command(name = "decktrace", about = "斗地主记牌器", version)]
struct Args {
    #[arg(long, default_value = "jj")]
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
    let game = get_game(&args.game).map_err(anyhow::Error::msg)?.clone();
    let selector = window_selector(&args, &game);

    let (tx, rx) = bounded::<Message>(1);
    let stop = Arc::new(AtomicBool::new(false));
    let reset = Arc::new(AtomicBool::new(false));
    spawn_worker(
        game.clone(),
        selector,
        tx,
        rx.clone(),
        stop.clone(),
        reset.clone(),
        args.min_hits,
        args.fps,
        !args.no_auto_reset,
    )?;

    let tracker = CardTracker::new(Arc::new(game.clone()), args.min_hits, 0.92, 1.6);
    let initial = tracker.snapshot();
    let app = DeckApp::new(game, rx, stop, reset, initial);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("斗地主记牌器")
            .with_inner_size([
                (CARD_W * 54 + MARGIN * 2) as f32,
                (CARD_H + MARGIN * 2) as f32,
            ])
            .with_resizable(false)
            .with_always_on_top(),
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

fn spawn_worker(
    game: GameProfile,
    selector: WindowSelector,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    min_hits: usize,
    fps: f32,
    auto_reset: bool,
) -> Result<()> {
    std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let platform = create_platform()?;
                let window = platform.window_service().find_window(&selector)?;
                let recognizer: SharedRecognizer =
                    Arc::new(JjRecognizer::new(&game).map_err(|err| anyhow::anyhow!("{err}"))?);
                let mut session = Session::new(
                    platform.as_ref(),
                    game,
                    &window,
                    recognizer,
                    SessionConfig {
                        fps,
                        min_hits,
                        auto_reset,
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
    game: GameProfile,
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    snapshot: TrackerSnapshot,
    error: Option<String>,
    sources: HashMap<String, RgbaImage>,
    textures: HashMap<(String, bool), egui::TextureHandle>,
    badge: Option<egui::TextureHandle>,
    last_key: Option<(Vec<String>, BTreeSet<String>)>,
}

impl DeckApp {
    fn new(
        game: GameProfile,
        rx: Receiver<Message>,
        stop: Arc<AtomicBool>,
        reset: Arc<AtomicBool>,
        snapshot: TrackerSnapshot,
    ) -> Self {
        Self {
            game,
            rx,
            stop,
            reset,
            snapshot,
            error: None,
            sources: HashMap::new(),
            textures: HashMap::new(),
            badge: None,
            last_key: None,
        }
    }

    fn drain(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Snapshot(snapshot) => self.snapshot = snapshot,
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
            let path = self.game.card_assets_dir.join(format!("{card_id}.png"));
            let image = image::open(path).ok()?.to_rgba8();
            self.sources.insert(card_id.to_string(), image);
        }
        self.sources.get(card_id)
    }
}

impl eframe::App for DeckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        let ctx = ui.ctx().clone();
        if ctx.input(|input| input.key_pressed(egui::Key::R)) {
            self.reset.store(true, Ordering::Relaxed);
        }
        let remaining = self.game.remaining_card_ids(&self.snapshot.seen_ids);
        let bombs = bomb_card_ids(&remaining, &self.game);
        let cards: Vec<(String, bool)> = remaining
            .iter()
            .map(|card_id| (card_id.clone(), bombs.contains(card_id)))
            .collect();

        let key = (remaining.clone(), bombs.clone());
        if self.last_key.as_ref() != Some(&key) {
            self.last_key = Some(key);
            self.ensure_textures(&ctx, &cards);
        }

        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(241, 236, 225));
        if self.error.is_some() {
            // egui 默认字体没有中文，错误详情已经在 worker 里输出到 stderr。
            ui.label("Screen capture failed - check Screen Recording permission.");
            return;
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
            if self.snapshot.bottom_ids.contains(card_id)
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
