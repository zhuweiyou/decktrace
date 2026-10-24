use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use image::imageops::FilterType;
use image::{GrayImage, ImageBuffer, Luma, Rgb, RgbImage};
use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;
use imageproc::region_labelling::{Connectivity, connected_components};
use imageproc::template_matching::{MatchTemplateMethod, match_template_parallel};
use thiserror::Error;

use decktrace_core::{Detection, RecognitionConfig, SUIT_SYMBOLS};
use decktrace_game_api::{BottomSpec, GameProfile};

#[derive(Debug, Error)]
pub enum RecognitionError {
    #[error("缺少模板目录：{0}")]
    MissingTemplateDir(String),
    #[error("点数或花色模板不完整")]
    IncompleteTemplates,
    #[error("读取模板失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("解码图片失败：{0}")]
    Image(#[from] image::ImageError),
}

#[derive(Debug, Clone)]
pub struct RecognitionResult {
    pub detections: Vec<Detection>,
    pub elapsed_ms: f64,
    pub frame_width: u32,
    pub frame_height: u32,
}

#[derive(Debug, Clone, Copy)]
struct Comp {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    area: u32,
}

pub struct CardRecognizer {
    cfg: RecognitionConfig,
    ranks: BTreeMap<String, GrayImage>,
    suits: BTreeMap<String, GrayImage>,
    joker_templates: BTreeMap<String, GrayImage>,
    labels: BTreeMap<String, String>,
    bottom: Option<Arc<dyn BottomSpec>>,
}

impl CardRecognizer {
    pub fn new(game: &GameProfile) -> Result<Self, RecognitionError> {
        let base = Path::new(&game.templates_dir);
        let rank_dir = base.join("ranks");
        let suit_dir = base.join("suits");
        if !rank_dir.exists() || !suit_dir.exists() {
            return Err(RecognitionError::MissingTemplateDir(format!(
                "{} / {}",
                rank_dir.display(),
                suit_dir.display()
            )));
        }

        let ranks = load_templates(&rank_dir)?;
        let suits = load_templates(&suit_dir)?;
        if ranks.is_empty() || suits.len() != 4 {
            return Err(RecognitionError::IncompleteTemplates);
        }

        let mut joker_templates = BTreeMap::new();
        for name in [
            Some(game.recognition.joker_template.clone()),
            game.recognition.joker_compact_template.clone(),
        ]
        .into_iter()
        .flatten()
        {
            let path = base.join(&name);
            if path.exists() {
                joker_templates.insert(name, image::open(path)?.to_luma8());
            }
        }

        Ok(Self {
            cfg: game.recognition.clone(),
            ranks,
            suits,
            joker_templates,
            labels: game.labels.clone(),
            bottom: game.bottom.clone(),
        })
    }

    pub fn recognize(&self, image: &RgbImage, detect_jokers: bool) -> RecognitionResult {
        let started = std::time::Instant::now();
        let timing = std::env::var("DECKTRACE_TIMING").is_ok();
        let (width, height) = image.dimensions();
        let (red, dark) = masks(image);
        let combined = combine(&red, &dark);
        let t_masks = started.elapsed();
        let (mut kept, jokers) = rayon::join(
            || self.detect_cards(image, &red, &dark),
            || {
                if detect_jokers {
                    self.detect_jokers(image, &combined, &red, &dark)
                } else {
                    Vec::new()
                }
            },
        );
        kept.extend(jokers);
        if timing {
            eprintln!(
                "[timing] masks={:.0}ms total={:.0}ms",
                t_masks.as_secs_f64() * 1000.0,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }

        let mut kept = if let Some(bottom) = &self.bottom {
            bottom.mark(image, kept)
        } else {
            kept
        };
        kept.sort_by_key(|d| (d.bbox.1 / 100, d.bbox.0));

        RecognitionResult {
            detections: kept,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            frame_width: width,
            frame_height: height,
        }
    }

    fn detect_cards(&self, image: &RgbImage, red: &GrayImage, dark: &GrayImage) -> Vec<Detection> {
        // 红/黑两套掩码互不依赖，并行处理。
        let (mut red_raw, mut black_raw) = rayon::join(
            || self.detect_color(image, red, "red"),
            || self.detect_color(image, dark, "black"),
        );
        red_raw.append(&mut black_raw);
        let mut raw = red_raw;
        raw.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Detection> = Vec::new();
        for det in raw {
            let (x, y, w, h) = det.bbox;
            let cx = x as f32 + w as f32 / 2.0;
            let cy = y as f32 + h as f32 / 2.0;
            let overlaps = kept.iter().any(|k| {
                let (kx, ky, kw, kh) = k.bbox;
                let kcx = kx as f32 + kw as f32 / 2.0;
                let kcy = ky as f32 + kh as f32 / 2.0;
                (cx - kcx).abs() < (w.min(kw) as f32) * 0.6
                    && (cy - kcy).abs() < (h.min(kh) as f32) * 0.6
            });
            if !overlaps {
                kept.push(det);
            }
        }
        kept
    }

    fn detect_color(&self, image: &RgbImage, mask: &GrayImage, color: &str) -> Vec<Detection> {
        let mut raw = Vec::new();
        for comp in components(mask) {
            if !(12..=80).contains(&comp.w) || !(12..=85).contains(&comp.h) {
                continue;
            }
            let aspect = comp.w as f32 / comp.h as f32;
            if !(0.65..=1.45).contains(&aspect) {
                continue;
            }
            let fill = comp.area as f32 / (comp.w * comp.h) as f32;
            if fill < 0.28 || comp.area < 120 {
                continue;
            }
            let Some((rx, ry, rw, rh)) = rank_bbox(mask, comp.x, comp.y, comp.w, comp.h) else {
                continue;
            };
            let rank_crop = crop(mask, rx, ry, rw, rh);
            let (rank, rank_score, second_score) = self.classify_rank(&rank_crop);
            let suit_crop = crop(mask, comp.x, comp.y, comp.w, comp.h);
            let (suit, suit_score) = self.classify_suit(&suit_crop, color);
            if rank_score < self.cfg.rank_min_score
                || suit_score < self.cfg.suit_min_score
                || rank_score - second_score < self.cfg.rank_margin
            {
                continue;
            }
            let face = face_fraction(image, comp.x, comp.y, comp.w, comp.h);
            if face < self.cfg.face_min_fraction {
                continue;
            }
            raw.push(Detection {
                rank: rank.clone(),
                suit: suit.clone(),
                score: rank_score * 0.7 + suit_score * 0.3,
                bbox: (comp.x, comp.y, comp.w, comp.h),
                rank_score,
                suit_score,
                face_score: face,
                display: self.label(&rank, &suit),
                is_bottom: false,
            });
        }
        raw
    }

    fn label(&self, rank: &str, suit: &str) -> String {
        if let Some(label) = self.labels.get(rank) {
            return label.clone();
        }
        if let Some((_, symbol)) = SUIT_SYMBOLS.iter().find(|(s, _)| *s == suit) {
            format!("{rank}{symbol}")
        } else {
            rank.to_string()
        }
    }

    fn classify_rank(&self, crop: &GrayImage) -> (String, f32, f32) {
        if crop.width() == 0 || crop.height() == 0 {
            return (String::new(), 0.0, 0.0);
        }
        let normalized = normalize_rank(crop, self.cfg.rank_template_height);
        let mut scores: Vec<(String, f32)> = self
            .ranks
            .iter()
            .map(|(name, template)| {
                let resized = image::imageops::resize(
                    &normalized,
                    template.width(),
                    template.height(),
                    FilterType::Triangle,
                );
                let a = binarize_owned(resized);
                (name.clone(), iou(&a, template))
            })
            .collect();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        let best = scores.first().cloned().unwrap_or_default();
        let second = scores.get(1).map(|s| s.1).unwrap_or(0.0);
        (best.0, best.1, second)
    }

    fn classify_suit(&self, crop: &GrayImage, color: &str) -> (String, f32) {
        if crop.width() == 0 || crop.height() == 0 {
            return (String::new(), 0.0);
        }
        let normalized = normalize_suit(crop);
        let options: &[&str] = if color == "red" {
            &["H", "D"]
        } else {
            &["S", "C"]
        };
        let mut best_name = String::new();
        let mut best_score = 0.0f32;
        for name in options {
            if let Some(template) = self.suits.get(*name) {
                let score = iou(&normalized, template);
                if score > best_score {
                    best_name = (*name).to_string();
                    best_score = score;
                }
            }
        }
        (best_name, best_score)
    }

    fn detect_jokers(
        &self,
        image: &RgbImage,
        combined: &GrayImage,
        red: &GrayImage,
        dark: &GrayImage,
    ) -> Vec<Detection> {
        use rayon::prelude::*;

        if self.joker_templates.is_empty() {
            return Vec::new();
        }
        let width = combined.width();
        let height = combined.height();
        let base_scale = width as f32 / self.cfg.reference_width.max(1) as f32;

        // 每个高度带只裁一次；不同尺度并行。
        let mut searches = Vec::with_capacity(self.cfg.joker_bands.len());
        for band in &self.cfg.joker_bands {
            let y0 = (height as f32 * band.top_ratio) as u32;
            let y1 = ((height as f32 * band.bottom_ratio) as u32).min(height);
            if y1 <= y0 + 10 {
                searches.push(None);
            } else {
                searches.push(Some((y0, crop(combined, 0, y0, width, y1 - y0))));
            }
        }

        let tasks: Vec<(usize, f32)> = self
            .cfg
            .joker_bands
            .iter()
            .enumerate()
            .flat_map(|(index, band)| band.scales.iter().map(move |s| (index, *s)))
            .collect();

        let groups: Vec<Vec<Detection>> = tasks
            .par_iter()
            .map(|(band_index, prior)| {
                let Some((y0, search)) = &searches[*band_index] else {
                    return Vec::new();
                };
                let band = &self.cfg.joker_bands[*band_index];
                let Some(source) = self.joker_templates.get(&band.template) else {
                    return Vec::new();
                };
                let base_h = source.height() as f32;
                let base_w = source.width() as f32;
                let scale = *prior * base_scale;
                let template_h = ((base_h * scale).round() as u32).max(12);
                let template_w = ((base_w * template_h as f32 / base_h).round() as u32).max(6);
                if template_h >= search.height() || template_w >= search.width() {
                    return Vec::new();
                }
                let template_full =
                    image::imageops::resize(source, template_w, template_h, FilterType::Triangle);
                let template_full = binarize_owned(template_full);

                let mut found = Vec::new();
                for (x, y, score) in
                    coarse_candidates(search, &template_full, self.cfg.joker_min_score)
                {
                    let fx = x.min(width.saturating_sub(template_w));
                    let fy = (*y0 + y).min(height.saturating_sub(template_h));
                    let red_count = count_nonzero(red, fx, fy, template_w, template_h);
                    let dark_count = count_nonzero(dark, fx, fy, template_w, template_h);
                    if red_count.max(dark_count) < 50 {
                        continue;
                    }
                    let rank = if red_count > dark_count { "BJ" } else { "SJ" };
                    let face = face_fraction(image, fx, fy, template_w, template_h);
                    if face < self.cfg.face_min_fraction {
                        continue;
                    }
                    found.push(Detection {
                        rank: rank.to_string(),
                        suit: String::new(),
                        score,
                        bbox: (fx, fy, template_w, template_h),
                        rank_score: score,
                        suit_score: 1.0,
                        face_score: face,
                        display: self.label(rank, ""),
                        is_bottom: false,
                    });
                }
                found
            })
            .collect();

        let mut detections: Vec<Detection> = groups.into_iter().flatten().collect();
        detections.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Detection> = Vec::new();
        for det in detections {
            let (x, y, w, h) = det.bbox;
            let cx = x as f32 + w as f32 / 2.0;
            let cy = y as f32 + h as f32 / 2.0;
            let overlaps = kept.iter().any(|k| {
                let (kx, ky, kw, kh) = k.bbox;
                let kcx = kx as f32 + kw as f32 / 2.0;
                let kcy = ky as f32 + kh as f32 / 2.0;
                (cx - kcx).abs() < (w.max(kw) as f32) * 0.35
                    && (cy - kcy).abs() < (h.max(kh) as f32) * 0.25
            });
            if !overlaps {
                kept.push(det);
            }
        }
        kept
    }
}

fn load_templates(dir: &Path) -> Result<BTreeMap<String, GrayImage>, RecognitionError> {
    let mut templates = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        templates.insert(stem.to_string(), image::open(path)?.to_luma8());
    }
    Ok(templates)
}

fn masks(image: &RgbImage) -> (GrayImage, GrayImage) {
    let (width, height) = image.dimensions();
    let mut red = GrayImage::new(width, height);
    let mut dark = GrayImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b] = pixel.0;
        let is_red = r > 110 && (r as i32 - g as i32) > 45 && (r as i32 - b as i32) > 35;
        let is_dark = r < 90 && g < 90 && b < 90;
        red.put_pixel(x, y, Luma([if is_red { 255 } else { 0 }]));
        dark.put_pixel(x, y, Luma([if is_dark { 255 } else { 0 }]));
    }
    (red, dark)
}

fn combine(red: &GrayImage, dark: &GrayImage) -> GrayImage {
    ImageBuffer::from_fn(red.width(), red.height(), |x, y| {
        let r = red.get_pixel(x, y).0[0];
        let d = dark.get_pixel(x, y).0[0];
        Luma([r.max(d)])
    })
}

fn components(mask: &GrayImage) -> Vec<Comp> {
    let labels = connected_components(mask, Connectivity::Eight, Luma([0u8]));
    let mut table: Vec<(u32, u32, u32, u32, u32)> = Vec::new();
    for (x, y, label) in labels.enumerate_pixels() {
        let id = label.0[0] as usize;
        if id == 0 {
            continue;
        }
        if table.len() <= id {
            table.resize(id + 1, (u32::MAX, u32::MAX, 0, 0, 0));
        }
        let entry = &mut table[id];
        entry.0 = entry.0.min(x);
        entry.1 = entry.1.min(y);
        entry.2 = entry.2.max(x);
        entry.3 = entry.3.max(y);
        entry.4 += 1;
    }
    table
        .into_iter()
        .filter(|entry| entry.4 > 0)
        .map(|(x0, y0, x1, y1, area)| Comp {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
            area,
        })
        .collect()
}

fn rank_bbox(mask: &GrayImage, sx: u32, sy: u32, sw: u32, sh: u32) -> Option<(u32, u32, u32, u32)> {
    let y0 = sy.saturating_sub((2.2 * sh as f32) as u32);
    let y1 = sy.saturating_sub(2);
    if y1 <= y0 {
        return None;
    }
    let x0 = sx.saturating_sub((0.2 * sw as f32) as u32);
    let x1 = (sx + (1.45 * sw as f32) as u32).min(mask.width());
    if x1 <= x0 {
        return None;
    }
    let sub = crop(mask, x0, y0, x1 - x0, y1 - y0);
    let min_h = (0.2 * sh as f32).max(8.0) as u32;
    let max_h = (2.0 * sh as f32) as u32;
    let min_area = (0.01 * sh as f32 * sh as f32).max(20.0) as u32;
    let mut glyphs: Vec<Comp> = Vec::new();
    for comp in components(&sub) {
        if comp.h < min_h || comp.h > max_h {
            continue;
        }
        if comp.w > (1.8 * sh as f32) as u32 || comp.area < min_area {
            continue;
        }
        if comp.y == 0 && comp.w > (0.8 * sh as f32) as u32 {
            continue;
        }
        glyphs.push(comp);
    }
    if glyphs.is_empty() {
        return None;
    }
    let x = glyphs.iter().map(|c| c.x).min()?;
    let y = glyphs.iter().map(|c| c.y).min()?;
    let x2 = glyphs.iter().map(|c| c.x + c.w).max()?;
    let y2 = glyphs.iter().map(|c| c.y + c.h).max()?;
    Some((x0 + x, y0 + y, x2 - x, y2 - y))
}

fn crop(image: &GrayImage, x: u32, y: u32, w: u32, h: u32) -> GrayImage {
    image::imageops::crop_imm(image, x, y, w, h).to_image()
}

fn normalize_rank(crop: &GrayImage, target_h: u32) -> GrayImage {
    let (w, h) = crop.dimensions();
    if h == 0 {
        return GrayImage::new(1, target_h);
    }
    let nw = ((w as f32 * target_h as f32 / h as f32).round() as u32).max(1);
    let resized = image::imageops::resize(crop, nw, target_h, FilterType::Triangle);
    binarize_owned(resized)
}

fn normalize_suit(crop: &GrayImage) -> GrayImage {
    const SIZE: u32 = 40;
    let (w, h) = crop.dimensions();
    let scale = (SIZE as f32 - 6.0) / w.max(h).max(1) as f32;
    let rw = ((w as f32 * scale).round() as u32).max(1);
    let rh = ((h as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(crop, rw, rh, FilterType::Triangle);
    let mut canvas = GrayImage::new(SIZE, SIZE);
    let ox = (SIZE - rw) / 2;
    let oy = (SIZE - rh) / 2;
    for (x, y, p) in resized.enumerate_pixels() {
        canvas.put_pixel(ox + x, oy + y, Luma([if p.0[0] > 96 { 255 } else { 0 }]));
    }
    canvas
}

fn binarize_owned(image: GrayImage) -> GrayImage {
    ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
        Luma([if image.get_pixel(x, y).0[0] > 96 {
            255
        } else {
            0
        }])
    })
}

fn iou(a: &GrayImage, b: &GrayImage) -> f32 {
    if a.dimensions() != b.dimensions() {
        return 0.0;
    }
    let mut inter = 0u64;
    let mut union = 0u64;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        let aa = pa.0[0] > 0;
        let bb = pb.0[0] > 0;
        if aa && bb {
            inter += 1;
        }
        if aa || bb {
            union += 1;
        }
    }
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

fn face_fraction(image: &RgbImage, x: u32, y: u32, w: u32, h: u32) -> f32 {
    let pad = 8u32;
    let x0 = x.saturating_sub(pad);
    let y0 = y.saturating_sub(pad);
    let x1 = (x + w + pad).min(image.width());
    let y1 = (y + h + pad).min(image.height());
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let mut okay = 0u64;
    let mut total = 0u64;
    for py in y0..y1 {
        for px in x0..x1 {
            let [r, g, b] = image.get_pixel(px, py).0;
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            if mx > 150 && (mx as i32 - mn as i32) < 35 {
                okay += 1;
            }
            total += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        okay as f32 / total as f32
    }
}

fn count_nonzero(mask: &GrayImage, x: u32, y: u32, w: u32, h: u32) -> u32 {
    let mut count = 0;
    for py in y..(y + h).min(mask.height()) {
        for px in x..(x + w).min(mask.width()) {
            if mask.get_pixel(px, py).0[0] > 0 {
                count += 1;
            }
        }
    }
    count
}

/// 粗搜索 + 局部精确 CCOEFF_NORMED，避免全图滑窗。
fn coarse_candidates(
    search: &GrayImage,
    template: &GrayImage,
    threshold: f32,
) -> Vec<(u32, u32, f32)> {
    let factor = 4u32;
    let sw = (search.width() / factor).max(1);
    let sh = (search.height() / factor).max(1);
    let tw = (template.width() / factor).max(1);
    let th = (template.height() / factor).max(1);
    if tw >= sw || th >= sh {
        return Vec::new();
    }
    let small_search = image::imageops::resize(search, sw, sh, FilterType::Triangle);
    let small_template = image::imageops::resize(template, tw, th, FilterType::Triangle);
    let small_template = binarize_owned(small_template);
    let result = match_template_parallel(
        &small_search,
        &small_template,
        MatchTemplateMethod::CrossCorrelationNormalized,
    );

    let mut candidates: Vec<(u32, u32, f32)> = Vec::new();
    for (x, y, p) in result.enumerate_pixels() {
        let score = p.0[0];
        if score < threshold {
            continue;
        }
        if candidates.iter().any(|(cx, cy, _)| {
            (*cx as i32 - x as i32).abs() < (tw as i32 / 2).max(2)
                && (*cy as i32 - y as i32).abs() < (th as i32 / 2).max(2)
        }) {
            continue;
        }
        candidates.push((x, y, score));
    }
    candidates.sort_by(|a, b| b.2.total_cmp(&a.2));
    candidates.truncate(8);

    let mut verified = Vec::new();
    for (x, y, _coarse_score) in candidates {
        let fx = (x * factor).min(search.width().saturating_sub(template.width()));
        let fy = (y * factor).min(search.height().saturating_sub(template.height()));
        let pad = factor * 2;
        let wx = fx.saturating_sub(pad);
        let wy = fy.saturating_sub(pad);
        let wx1 = (fx + template.width() + pad).min(search.width());
        let wy1 = (fy + template.height() + pad).min(search.height());
        if wx1 <= wx + template.width() || wy1 <= wy + template.height() {
            continue;
        }
        let window = crop(search, wx, wy, wx1 - wx, wy1 - wy);
        if let Some((ox, oy, score)) = best_ccoeff(&window, template) {
            if score >= threshold {
                verified.push((wx + ox, wy + oy, score));
            }
        }
    }
    verified
}

fn best_ccoeff(search: &GrayImage, template: &GrayImage) -> Option<(u32, u32, f32)> {
    if template.width() > search.width() || template.height() > search.height() {
        return None;
    }
    let n = (template.width() * template.height()) as f32;
    let mut t_sum = 0.0f64;
    for p in template.pixels() {
        t_sum += p.0[0] as f64;
    }
    let t_mean = t_sum / n as f64;
    let mut t_var = 0.0f64;
    for p in template.pixels() {
        let v = p.0[0] as f64 - t_mean;
        t_var += v * v;
    }
    if t_var == 0.0 {
        return None;
    }

    let tw = template.width();
    let th = template.height();
    let mut best = (0u32, 0u32, f64::NEG_INFINITY);
    for y in 0..=(search.height() - th) {
        for x in 0..=(search.width() - tw) {
            let mut sum = 0.0f64;
            let mut sum2 = 0.0f64;
            let mut cross = 0.0f64;
            for ty in 0..th {
                for tx in 0..tw {
                    let i = search.get_pixel(x + tx, y + ty).0[0] as f64;
                    let t = template.get_pixel(tx, ty).0[0] as f64;
                    sum += i;
                    sum2 += i * i;
                    cross += i * t;
                }
            }
            let mean = sum / n as f64;
            let var = (sum2 - sum * mean).max(0.0);
            let cov = cross - t_mean * sum;
            let denom = (var * t_var).sqrt();
            let score = if denom > 1e-9 { cov / denom } else { 0.0 };
            if score > best.2 {
                best = (x, y, score);
            }
        }
    }
    if best.2.is_finite() {
        Some((best.0, best.1, best.2 as f32))
    } else {
        None
    }
}

pub fn annotate(image: &RgbImage, detections: &[Detection]) -> RgbImage {
    let mut out = image.clone();
    for det in detections {
        let (x, y, w, h) = det.bbox;
        if w == 0 || h == 0 {
            continue;
        }
        draw_hollow_rect_mut(
            &mut out,
            Rect::at(x as i32, y as i32).of_size(w, h),
            Rgb([0, 255, 0]),
        );
    }
    out
}
