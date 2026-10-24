use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossbeam_channel::{Receiver, Sender, bounded};
use decktrace_capture::Platform;
use decktrace_core::{CardTracker, TrackerSnapshot};
use decktrace_game_spec::GameProfile;
use decktrace_games::get_game;
use decktrace_session::Session;
use eframe::egui;
use image::{Rgba, RgbaImage};

const CARD_W: u32 = 15;
const CARD_H: u32 = 30;
const MARGIN: u32 = 4;

#[derive(Parser, Debug)]
#[command(name = "decktrace-gui", about = "斗地主记牌器（GUI）", version)]
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
}

enum Message {
    Snapshot(TrackerSnapshot),
    Error(String),
}

fn main() -> Result<()> {
    let args = Args::parse();
    let game = get_game(&args.game).map_err(anyhow::Error::msg)?.clone();
    let window = find_window(&args, &game)?;

    let (tx, rx) = bounded::<Message>(1);
    let stop = Arc::new(AtomicBool::new(false));
    spawn_worker(game.clone(), window, tx, rx.clone(), stop.clone())?;

    let tracker = CardTracker::new(game.deck_rules(), args.min_hits, 0.92, 1.6);
    let initial = tracker.snapshot();
    let app = DeckApp::new(game, rx, stop, initial);
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

fn find_window(args: &Args, game: &GameProfile) -> Result<decktrace_core::WindowInfo> {
    let platform = create_platform()?;
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
    platform
        .window_service()
        .find_window(&selector)
        .context("查找目标窗口失败")
}

fn spawn_worker(
    game: GameProfile,
    window: decktrace_core::WindowInfo,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let platform = create_platform()?;
                let mut session = Session::new(platform.as_ref(), game, &window, 15.0, 2, true)?;
                session.start()?;
                while !stop.load(Ordering::Relaxed) {
                    match session.tick(Duration::from_millis(200)) {
                        Ok(Some(tick)) => {
                            if tx.try_send(Message::Snapshot(tick.snapshot)).is_err() {
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
    snapshot: TrackerSnapshot,
    error: Option<String>,
    sources: HashMap<String, RgbaImage>,
    textures: HashMap<(String, u8), egui::TextureHandle>,
    last_key: Option<(Vec<String>, BTreeSet<String>)>,
}

impl DeckApp {
    fn new(
        game: GameProfile,
        rx: Receiver<Message>,
        stop: Arc<AtomicBool>,
        snapshot: TrackerSnapshot,
    ) -> Self {
        Self {
            game,
            rx,
            stop,
            snapshot,
            error: None,
            sources: HashMap::new(),
            textures: HashMap::new(),
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

    fn ensure_textures(&mut self, ctx: &egui::Context, cards: &[(String, u8)]) {
        for (card_id, state) in cards {
            let key = (card_id.clone(), *state);
            if self.textures.contains_key(&key) {
                continue;
            }
            let Some(source) = self.source(card_id) else {
                continue;
            };
            let image = match state {
                1 => tint_bottom(source),
                2 => invert(source),
                _ => source.clone(),
            };
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("{card_id}-{state}"),
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
        let remaining = self.game.remaining_card_ids(&self.snapshot.seen_ids);
        let bombs = bomb_card_ids(&remaining, &self.game);
        let cards: Vec<(String, u8)> = remaining
            .iter()
            .map(|card_id| {
                let state = if self.snapshot.bottom_ids.contains(card_id) {
                    1
                } else if bombs.contains(card_id) {
                    2
                } else {
                    0
                };
                (card_id.clone(), state)
            })
            .collect();

        let key = (remaining.clone(), self.snapshot.bottom_ids.clone());
        if self.last_key.as_ref() != Some(&key) {
            self.last_key = Some(key);
            self.ensure_textures(&ctx, &cards);
        }

        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(241, 236, 225));
        if let Some(error) = &self.error {
            ui.label(error);
            return;
        }
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                (CARD_W * 54 + MARGIN * 2) as f32,
                (CARD_H + MARGIN * 2) as f32,
            ),
            egui::Sense::hover(),
        );
        for (index, (card_id, state)) in cards.iter().enumerate() {
            let Some(texture) = self.textures.get(&(card_id.clone(), *state)) else {
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
        }
        ctx.request_repaint_after(Duration::from_millis(80));
    }
}

impl Drop for DeckApp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn tint_bottom(source: &RgbaImage) -> RgbaImage {
    let mut out = source.clone();
    for pixel in out.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let blend = |value: u8, tint: u8| -> u8 {
            ((value as f32) * 0.58 + (tint as f32) * 0.42).round() as u8
        };
        *pixel = Rgba([blend(r, 37), blend(g, 99), blend(b, 235), a]);
    }
    out
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

#[cfg(target_os = "macos")]
fn create_platform() -> Result<Box<dyn Platform>> {
    Ok(Box::new(decktrace_capture_macos::MacPlatform::new()?))
}

#[cfg(not(target_os = "macos"))]
fn create_platform() -> Result<Box<dyn Platform>> {
    Err(anyhow::anyhow!("当前只实现了 macOS 采集后端"))
}
