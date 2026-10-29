use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use image::imageops::FilterType;
use image::{GrayImage, ImageBuffer, Luma, Rgb, RgbImage};
use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;
use thiserror::Error;

use tracking::{
    BottomSpec, Detection, GameProfile, RecognitionConfig, RecognitionResult, Recognizer,
    SUIT_SYMBOLS,
};

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

type ComponentStats = Option<(u32, u32, u32, u32, u32)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Comp {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    area: u32,
}

#[derive(Default)]
struct Scratch {
    red: Vec<u8>,
    dark: Vec<u8>,
    combined: Vec<u8>,
}

pub struct WzddzRecognizer {
    cfg: RecognitionConfig,
    ranks: BTreeMap<String, GrayImage>,
    suits: BTreeMap<String, GrayImage>,
    joker_templates: BTreeMap<String, GrayImage>,
    labels: BTreeMap<String, String>,
    bottom: Option<Arc<dyn BottomSpec>>,
    scratch: Mutex<Scratch>,
}

impl WzddzRecognizer {
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
        if std::env::var("DECKTRACE_RANK").is_ok() {
            eprintln!("[tpl] rank_dir={} ranks={:?}", rank_dir.display(),
                ranks.iter().map(|(k,v)| (k.clone(), v.dimensions())).collect::<Vec<_>>());
        }
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
            scratch: Mutex::new(Scratch::default()),
        })
    }

    pub fn recognize(&self, image: &RgbImage, detect_jokers: bool) -> RecognitionResult {
        let started = std::time::Instant::now();
        let timing = std::env::var("DECKTRACE_TIMING").is_ok();
        let (width, height) = image.dimensions();
        let mut scratch = self
            .scratch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut red_buf = std::mem::take(&mut scratch.red);
        let mut dark_buf = std::mem::take(&mut scratch.dark);
        let mut combined_buf = std::mem::take(&mut scratch.combined);
        let (red, dark, combined) = masks(image, &mut red_buf, &mut dark_buf, &mut combined_buf);
        let t_masks = started.elapsed();
        let (mut kept, jokers) = rayon::join(
            || self.detect_cards(image, &red, &dark, &combined),
            || {
                if !detect_jokers {
                    return Vec::new();
                }
                if self.joker_templates.is_empty() {
                    // 欢乐斗地主没有“王”字：大小王是竖排 JOKER 字样，
                    // 用竖排字形聚类识别，红色=大王、黑色=小王。
                    self.detect_jokers_vertical(image, &red, &dark)
                } else {
                    self.detect_jokers(image, &combined, &red, &dark)
                }
            },
        );
        let jokers: Vec<Detection> = jokers
            .into_iter()
            .filter(|d| {
                let cx = d.bbox.0 as f32 + d.bbox.2 as f32 / 2.0;
                let cy = d.bbox.1 as f32 + d.bbox.3 as f32 / 2.0;
                // 王只会出现在「手牌带左侧（手牌按大小排，王在最左）」或「中间出牌区」，
                // 据此排除左右角色身体/服装图案造成的竖排误判。
                let cyr = cy / height as f32;
                let cxr = cx / width as f32;
                let in_hand = cyr > 0.66 && cyr < 0.96 && cxr > 0.10 && cxr < 0.58;
                let in_play = cyr > 0.16 && cyr < 0.60 && cxr > 0.22 && cxr < 0.82;
                let pos_ok = in_hand || in_play;
                if !pos_ok {
                    return false;
                }
                // 真正的王是白卡：区域平均亮度要高（排除角色立绘/特效误判）
                let (bx, by, bw, bh) = d.bbox;
                let gx = (bw / 8).max(1);
                let gy = (bh / 8).max(1);
                let mut sum = 0f32;
                let mut n = 0f32;
                let mut yy = by;
                while yy < by + bh {
                    let mut xx = bx;
                    while xx < bx + bw {
                        let px = image.get_pixel(xx.min(width - 1), yy.min(height - 1)).0;
                        sum += (px[0] as f32 + px[1] as f32 + px[2] as f32) / 3.0;
                        n += 1.0;
                        xx += gx;
                    }
                    yy += gy;
                }
                n > 0.0 && sum / n > 150.0
            })
            .collect();
        kept.extend(jokers);
        // 底牌由 detect_color 直接检出（区域标记 is_bottom），不再走额外低对比通路，避免重复。
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

        // 把掩码缓冲还回 scratch，下一帧复用。
        scratch.red = red.into_raw();
        scratch.dark = dark.into_raw();
        scratch.combined = combined.into_raw();

        RecognitionResult {
            detections: kept,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            frame_width: width,
            frame_height: height,
        }
    }

    fn detect_cards(&self, image: &RgbImage, red: &GrayImage, dark: &GrayImage, combined: &GrayImage) -> Vec<Detection> {
        // 红/黑两套掩码互不依赖，并行处理。
        let (mut red_raw, mut black_raw) = rayon::join(
            || self.detect_color(image, red, combined, "red"),
            || self.detect_color(image, dark, combined, "black"),
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

    fn detect_color(&self, image: &RgbImage, mask: &GrayImage, combined: &GrayImage, color: &str) -> Vec<Detection> {
        let mut raw = Vec::new();
        let dbg = std::env::var("DECKTRACE_COMPS").is_ok();
        let scale = (image.width() as f32 / self.cfg.reference_width.max(1) as f32).max(0.1);
        let min_side = (10.0 * scale) as u32;
        let max_side = (95.0 * scale).round() as u32;
        let min_area = (60.0 * scale * scale) as u32;
        let (iw, ih) = image.dimensions();
        for comp in components(mask) {
            let ccx = comp.x as f32 + comp.w as f32 / 2.0;
            let ccy = comp.y as f32 + comp.h as f32 / 2.0;
            // 顶部中间是底牌展示区：不计入已出（底牌属于地主手牌，未打出不应扣）。
            if ccy < ih as f32 * 0.13 && ccx > iw as f32 * 0.38 && ccx < iw as f32 * 0.82 {
                continue;
            }
            let _ = (iw, ih);
            if dbg {
                eprintln!("[comp] {} x={} y={} w={} h={} area={}", color, comp.x, comp.y, comp.w, comp.h, comp.area);
            }
            if comp.w < min_side || comp.w > max_side || comp.h < min_side || comp.h > max_side {
                continue;
            }
            let aspect = comp.w as f32 / comp.h as f32;
            if !(0.50..=1.60).contains(&aspect) {
                continue;
            }
            let fill = comp.area as f32 / (comp.w * comp.h) as f32;
            if fill < 0.15 || comp.area < min_area {
                continue;
            }
            let Some((rx, ry, rw, rh)) = rank_above(combined, comp.x, comp.y, comp.w, comp.h, scale) else {
                continue;
            };
            let rank_crop = crop(mask, rx, ry, rw, rh);
            if dbg && (comp.x == 649 || comp.x == 748 || comp.x == 553 || comp.x == 1939) {
                let _ = rank_crop.save(format!("/tmp/adbprobe/rc_{}.png", comp.x));
            }
            let (rank, rank_score, second_score) = self.classify_rank(&rank_crop);
            let suit_crop = crop(mask, comp.x, comp.y, comp.w, comp.h);
            let (suit, suit_score) = self.classify_suit(&suit_crop, color);
            let face = face_fraction(image, comp.x, comp.y, comp.w, comp.h);
            let (iw2, ih2) = image.dimensions();
            let ccx = comp.x as f32 + comp.w as f32 / 2.0;
            let ccy = comp.y as f32 + comp.h as f32 / 2.0;
            let in_bottom = ccx > iw2 as f32 * 0.40 && ccx < iw2 as f32 * 0.72
                && ccy > ih2 as f32 * 0.02 && ccy < ih2 as f32 * 0.15;
            let on_white = white_fraction(image, comp.x, comp.y, comp.w, comp.h, 0.6) > 0.30;
            let pass = if !on_white {
                false
            } else if in_bottom {
                rank_score >= 0.22 && suit_score >= 0.45 && face >= 0.15
            } else {
                rank_score >= self.cfg.rank_min_score
                    && suit_score >= self.cfg.suit_min_score
                    && (rank_score - second_score) >= self.cfg.rank_margin
                    && face >= self.cfg.face_min_fraction
            };
            if dbg {
                eprintln!("[card] {} x={} y={} w={} h={} rank={} rs={:.2} r2={:.2} suit={} ss={:.2} face={:.2} pass={}",
                    color, comp.x, comp.y, comp.w, comp.h, rank, rank_score, second_score, suit, suit_score, face, pass);
            }
            if !pass {
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

    /// 识别牌桌顶部的三张底牌（含已打出后的半透明样式）。
    /// 只在顶部小卡片区域工作，尺寸由亮列/亮行剖面确定，不依赖固定坐标。
    #[allow(dead_code)]
    fn detect_bottom_cards(&self, image: &RgbImage) -> Vec<Detection> {
        let (width, height) = image.dimensions();
        let bx0 = (width as f32 * 0.42) as u32;
        let bx1 = ((width as f32 * 0.72) as u32).min(width);
        let by0 = (height as f32 * 0.02) as u32;
        let by1 = ((height as f32 * 0.16) as u32).min(height);
        if bx1 <= bx0 + 90 || by1 <= by0 + 40 {
            return Vec::new();
        }
        let bright = |r: u8, g: u8, b: u8| r.min(g).min(b) > 165;

        // 亮列剖面 → 三张牌连成的横向区间。
        let mut col = vec![0u32; (bx1 - bx0) as usize];
        for y in by0..by1 {
            for x in bx0..bx1 {
                let [r, g, b] = image.get_pixel(x, y).0;
                if bright(r, g, b) {
                    col[(x - bx0) as usize] += 1;
                }
            }
        }
        let need = ((by1 - by0) as f32 * 0.12) as u32;
        let mut segments: Vec<(u32, u32)> = Vec::new();
        let mut start: Option<u32> = None;
        for (index, &count) in col.iter().enumerate() {
            let hit = count >= need;
            if hit && start.is_none() {
                start = Some(index as u32);
            } else if !hit && start.is_some() {
                let from = start.take().unwrap();
                if index as u32 - from >= 16 {
                    segments.push((from, index as u32));
                }
            }
        }
        if let Some(from) = start
            && col.len() as u32 - from >= 16
        {
            segments.push((from, col.len() as u32));
        }
        // 卡片之间有深色分隔线，亮列会断成几段；先桥接小间隙再整体聚类。
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for (from, to) in segments {
            match runs.last_mut() {
                Some(last) if from <= last.1 + 24 => last.1 = to,
                _ => runs.push((from, to)),
            }
        }
        let bscale = (width as f32 / 1800.0).max(0.2);
        let run_min = (100.0 * bscale) as u32;
        let run_max = (210.0 * bscale) as u32;
        let Some((run_x0, run_x1)) = runs
            .into_iter()
            .find(|(a, b)| (*a >= run_min && *b <= run_max) || ((b - a) >= run_min && (b - a) <= run_max))
        else {
            return Vec::new();
        };

        // 亮行剖面 → 卡片纵向范围。
        let mut rows = vec![0u32; (by1 - by0) as usize];
        for y in by0..by1 {
            for x in (bx0 + run_x0)..(bx0 + run_x1) {
                let [r, g, b] = image.get_pixel(x, y).0;
                if bright(r, g, b) {
                    rows[(y - by0) as usize] += 1;
                }
            }
        }
        // 阈值取严一点：圆角/描边行不算，避免把桌布带进卡片裁剪。
        let row_need = ((run_x1 - run_x0) as f32 * 0.85) as u32;
        let (Some(top), Some(bottom)) = (
            rows.iter().position(|&count| count >= row_need),
            rows.iter().rposition(|&count| count >= row_need),
        ) else {
            return Vec::new();
        };
        let (y0, y1) = (by0 + top as u32, by0 + bottom as u32 + 1);

        // 整条底牌区一起二值化，再按字形列聚类：不依赖卡片边界，避免描边粘连。
        let scale = 3u32;
        let (cw, ch) = (run_x1 - run_x0, y1 - y0);
        if cw < (90.0 * bscale) as u32 || ch < (40.0 * bscale) as u32 || cw * scale > width * 4 {
            return Vec::new();
        }
        let card = image::imageops::crop_imm(image, bx0 + run_x0, y0, cw, ch).to_image();
        let (bw, bh) = (cw * scale, ch * scale);
        let big = image::imageops::resize(&card, bw, bh, FilterType::Lanczos3);

        let mut lumas = Vec::with_capacity((bw * bh) as usize);
        let mut chromas = Vec::with_capacity((bw * bh) as usize);
        for p in big.pixels() {
            let [r, g, b] = p.0;
            lumas.push((u32::from(r) + u32::from(g) + u32::from(b)) / 3);
            chromas.push(u32::from(r.max(g).max(b)) - u32::from(r.min(g).min(b)));
        }
        let mut sorted = lumas.clone();
        sorted.sort_unstable();
        let bg = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
        let fg = sorted[sorted.len() / 20];
        if bg.saturating_sub(fg) < 12 {
            return Vec::new();
        }
        let mut mask = GrayImage::new(bw, bh);
        for y in 0..bh {
            for x in 0..bw {
                let index = (y * bw + x) as usize;
                let luma = lumas[index];
                let chroma = chromas[index];
                // 半透明字形：要么偏暗（灰字），要么带轻微色偏（粉字）。
                let glyph = luma + 22 < bg || (chroma >= 14 && luma + 6 < bg);
                mask.put_pixel(x, y, Luma([if glyph { 255 } else { 0 }]));
            }
        }
        // 卡片描边是贯穿整条的高列/宽行，清掉，避免和字形连成一个分量。
        let mut columns = vec![0u32; bw as usize];
        let mut rows = vec![0u32; bh as usize];
        for (x, y, p) in mask.enumerate_pixels() {
            if p.0[0] > 0 {
                columns[x as usize] += 1;
                rows[y as usize] += 1;
            }
        }
        for (x, count) in columns.iter().enumerate() {
            if *count as f32 > bh as f32 * 0.72 {
                for y in 0..bh {
                    mask.put_pixel(x as u32, y, Luma([0]));
                }
            }
        }
        for (y, count) in rows.iter().enumerate() {
            if *count as f32 > bw as f32 * 0.8 {
                for x in 0..bw {
                    mask.put_pixel(x, y as u32, Luma([0]));
                }
            }
        }

        let comps: Vec<Comp> = components(&mask)
            .into_iter()
            .filter(|c| {
                c.area >= 40
                    && c.w >= 6
                    && c.h >= 8
                    && c.w <= (bw as f32 * 0.55) as u32
                    && c.h <= (bh as f32 * 0.65) as u32
            })
            .collect();
        if comps.is_empty() {
            return Vec::new();
        }

        // 按 x 重叠聚成“牌列”，每列上方是点数、下方是花色。
        let mut columns_of_comps: Vec<Vec<Comp>> = Vec::new();
        for comp in comps {
            let placed = columns_of_comps.iter_mut().find(|column| {
                let min_x = column.iter().map(|c| c.x).min().unwrap();
                let max_x = column.iter().map(|c| c.x + c.w).max().unwrap();
                let overlap = (comp.x + comp.w).min(max_x).saturating_sub(comp.x.max(min_x));
                overlap * 2 >= comp.w.min(max_x - min_x)
            });
            match placed {
                Some(column) => column.push(comp),
                None => columns_of_comps.push(vec![comp]),
            }
        }
        columns_of_comps.sort_by_key(|column| column.iter().map(|c| c.x).min().unwrap());

        let mut found = Vec::new();
        for column in columns_of_comps {
            let center = (bh as f32 * 0.5) as u32;
            let tops: Vec<Comp> = column
                .iter()
                .filter(|c| c.y + c.h / 2 < center)
                .copied()
                .collect();
            if tops.is_empty() {
                continue;
            }
            let rx0 = tops.iter().map(|c| c.x).min().unwrap();
            let ry0 = tops.iter().map(|c| c.y).min().unwrap();
            let rx1 = tops.iter().map(|c| c.x + c.w).max().unwrap();
            let ry1 = tops.iter().map(|c| c.y + c.h).max().unwrap();
            let rank_crop = crop(&mask, rx0, ry0, rx1 - rx0, ry1 - ry0);
            let (rank, rank_score, second_score) = self.classify_rank(&rank_crop);
            if rank_score < 0.52 || rank_score - second_score < 0.05 {
                continue;
            }

            let mut best_suit = (String::new(), 0.0f32, 0u32, 0u32, 0u32, 0u32);
            for c in column.iter().filter(|c| c.y + c.h / 2 >= center) {
                let aspect = c.w as f32 / c.h.max(1) as f32;
                if !(0.45..=1.9).contains(&aspect) {
                    continue;
                }
                let suit_crop = crop(&mask, c.x, c.y, c.w, c.h);
                for color in ["red", "black"] {
                    let (name, score) = self.classify_suit(&suit_crop, color);
                    if score > best_suit.1 {
                        best_suit = (name, score, c.x, c.y, c.w, c.h);
                    }
                }
            }
            if best_suit.0.is_empty() || best_suit.1 < 0.52 {
                continue;
            }

            found.push(Detection {
                rank: rank.clone(),
                suit: best_suit.0.clone(),
                score: rank_score * 0.6 + best_suit.1 * 0.4,
                bbox: (
                    bx0 + run_x0 + best_suit.2 / scale,
                    y0 + best_suit.3 / scale,
                    best_suit.4 / scale,
                    best_suit.5 / scale,
                ),
                rank_score,
                suit_score: best_suit.1,
                face_score: 1.0,
                display: self.label(&rank, &best_suit.0),
                is_bottom: true,
            });
        }
        found
    }

    /// 竖排 JOKER 字形识别：五个旋转字母在同一竖列上，红色为大王，黑色为小王。
    fn detect_jokers_vertical(
        &self,
        image: &RgbImage,
        red: &GrayImage,
        dark: &GrayImage,
    ) -> Vec<Detection> {
        // 尺寸阈值按参考宽度标定，手机等更高分辨率要按比例放大，否则竖排
        // JOKER 字母（比 PC 大一圈）会被尺寸过滤掉。
        let scale = (image.width() as f32 / self.cfg.reference_width.max(1) as f32).max(0.1);
        let letter_min = (8.0 * scale) as u32;
        let letter_max = (46.0 * scale).round() as u32;
        let area_min = (40.0 * scale * scale) as u32;
        let area_max = (1600.0 * scale * scale) as u32;
        let mut letters: Vec<(Comp, bool)> = Vec::new();
        for (mask, is_red) in [(red, true), (dark, false)] {
            for comp in components(mask) {
                // 竖排字母是窄小方块；牌面小丑插画会被红色掩码带进来，按尺寸剔掉。
                if !(letter_min..=letter_max).contains(&comp.w)
                    || !(letter_min..=letter_max).contains(&comp.h)
                {
                    continue;
                }
                if !(area_min..=area_max).contains(&comp.area) {
                    continue;
                }
                letters.push((comp, is_red));
            }
        }
        if letters.len() < 4 {
            return Vec::new();
        }
        letters.sort_by_key(|(comp, _)| comp.x);

        // 按 x 重合聚成竖列，再在列内按 y 找连续字母串。
        let mut columns: Vec<Vec<(Comp, bool)>> = Vec::new();
        for item in letters {
            let (comp, _) = &item;
            let placed = columns.iter_mut().find(|column| {
                let min_x = column.iter().map(|(c, _)| c.x).min().unwrap();
                let max_x = column.iter().map(|(c, _)| c.x + c.w).max().unwrap();
                let overlap = (comp.x + comp.w).min(max_x).saturating_sub(comp.x.max(min_x));
                overlap * 2 >= comp.w.min(max_x - min_x)
            });
            match placed {
                Some(column) => column.push(item),
                None => columns.push(vec![item]),
            }
        }

        let mut found = Vec::new();
        for mut column in columns {
            column.sort_by_key(|(comp, _)| comp.y);
            let mut run: Vec<(Comp, bool)> = Vec::new();
            for item in column {
                let (comp, _) = &item;
                let gap = run
                    .last()
                    .map(|(last, _)| comp.y.saturating_sub(last.y + last.h))
                    .unwrap_or(0);
                if !run.is_empty() && gap > (comp.h as f32 * 1.2) as u32 + (8.0 * scale) as u32 {
                    if let Some(det) = self.joker_from_run(image, red, dark, &run, scale) {
                        found.push(det);
                    }
                    run.clear();
                }
                run.push(item);
            }
            if let Some(det) = self.joker_from_run(image, red, dark, &run, scale) {
                found.push(det);
            }
        }
        found
    }

    fn joker_from_run(
        &self,
        image: &RgbImage,
        red: &GrayImage,
        dark: &GrayImage,
        run: &[(Comp, bool)],
        scale: f32,
    ) -> Option<Detection> {
        if run.len() < 4 {
            return None;
        }
        let x0 = run.iter().map(|(c, _)| c.x).min()?;
        let y0 = run.iter().map(|(c, _)| c.y).min()?;
        let x1 = run.iter().map(|(c, _)| c.x + c.w).max()?;
        let y1 = run.iter().map(|(c, _)| c.y + c.h).max()?;
        let (w, h) = (x1 - x0, y1 - y0);
        let h_min = (100.0 * scale) as u32;
        let h_max = (420.0 * scale) as u32;
        let w_min = (12.0 * scale) as u32;
        let w_max = (60.0 * scale) as u32;
        if !(h_min..=h_max).contains(&h) || w < w_min || w > w_max {
            return None;
        }
        // 真正的 JOKER 是细长竖列：高度 ≥ 宽度*4（普通牌的红桃/蝴蝶结更宽，会被排除）。
        if (h as f32) < (w as f32) * 4.0 {
            return None;
        }
        // 标题栏在这个高度以上，真正的王只出现在手牌或出牌区。
        if y0 < (image.height() as f32 * 0.06) as u32 {
            return None;
        }
        // 真正的王是白卡：要求白色卡面占比高，排除衣服/装饰造成的竖排误判。
        if white_fraction(image, x0, y0, w, h, 0.4) < 0.15 {
            return None;
        }
        let red_pixels = count_nonzero(red, x0, y0, w, h);
        let dark_pixels = count_nonzero(dark, x0, y0, w, h);
        if red_pixels.max(dark_pixels) < (200.0 * scale * scale) as u32 {
            return None;
        }
        let face = face_fraction(image, x0, y0, w, h);
        if face < self.cfg.face_min_fraction {
            return None;
        }
        let rank = if red_pixels > dark_pixels { "BJ" } else { "SJ" };
        Some(Detection {
            rank: rank.to_string(),
            suit: String::new(),
            score: 0.9,
            bbox: (x0, y0, w, h),
            rank_score: 0.9,
            suit_score: 1.0,
            face_score: face,
            display: self.label(rank, ""),
            is_bottom: false,
        })
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
                let filter = if template.width() >= normalized.width() {
                    FilterType::Nearest
                } else {
                    FilterType::Triangle
                };
                let resized = image::imageops::resize(
                    &normalized,
                    template.width(),
                    template.height(),
                    filter,
                );
                let a = binarize_owned(resized);
                (name.clone(), iou(&a, template))
            })
            .collect();
        if std::env::var("DECKTRACE_RANK").is_ok() {
            eprintln!("[rank] crop {}x{} scores {:?}", crop.width(), crop.height(),
                scores.iter().map(|(n, v)| (n.clone(), (v * 100.0).round() / 100.0)).collect::<Vec<_>>());
        }
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

impl Recognizer for WzddzRecognizer {
    fn recognize(&self, image: &RgbImage, detect_jokers: bool) -> RecognitionResult {
        WzddzRecognizer::recognize(self, image, detect_jokers)
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

fn masks(
    image: &RgbImage,
    red: &mut Vec<u8>,
    dark: &mut Vec<u8>,
    combined: &mut Vec<u8>,
) -> (GrayImage, GrayImage, GrayImage) {
    use rayon::prelude::*;

    let (width, height) = image.dimensions();
    let len = (width as usize) * (height as usize);
    if red.len() != len {
        red.resize(len, 0);
    }
    if dark.len() != len {
        dark.resize(len, 0);
    }
    if combined.len() != len {
        combined.resize(len, 0);
    }
    red.par_iter_mut()
        .zip(dark.par_iter_mut())
        .zip(combined.par_iter_mut())
        .zip(image.as_raw().par_chunks_exact(3))
        .for_each(|(((red_px, dark_px), combined_px), rgb)| {
            let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
            // 托管/暗态下手牌整体压暗，红色会掉饱和度；阈值放宽到 30/25，
            // 靠点数+花色+卡面三重校验过滤误检。
            let is_red = r > 100 && (r as i32 - g as i32) > 30 && (r as i32 - b as i32) > 25;
            let is_dark = r < 90 && g < 90 && b < 90;
            *red_px = if is_red { 255 } else { 0 };
            *dark_px = if is_dark { 255 } else { 0 };
            *combined_px = (*red_px).max(*dark_px);
        });
    (
        GrayImage::from_raw(width, height, std::mem::take(red)).expect("red mask"),
        GrayImage::from_raw(width, height, std::mem::take(dark)).expect("dark mask"),
        GrayImage::from_raw(width, height, std::mem::take(combined)).expect("combined mask"),
    )
}

fn components(mask: &GrayImage) -> Vec<Comp> {
    // 行程编码 + 两趟并查集。二值掩码很稀疏，比逐像素通用 union-find 快很多。
    #[derive(Clone, Copy)]
    struct Run {
        x0: u32,
        x1: u32,
        label: usize,
    }

    let width = mask.width() as usize;
    let height = mask.height() as usize;
    let raw = mask.as_raw();
    let mut parent: Vec<usize> = Vec::new();
    let mut rows: Vec<Vec<Run>> = Vec::with_capacity(height);
    let mut prev: Vec<Run> = Vec::new();

    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    }

    for y in 0..height {
        let row = &raw[y * width..(y + 1) * width];
        let mut current: Vec<Run> = Vec::new();
        let mut x = 0usize;
        while x < width {
            while x < width && row[x] == 0 {
                x += 1;
            }
            if x >= width {
                break;
            }
            let start = x;
            while x < width && row[x] != 0 {
                x += 1;
            }
            let label = parent.len();
            parent.push(label);
            current.push(Run {
                x0: start as u32,
                x1: (x - 1) as u32,
                label,
            });
        }

        // 8 连通：上一行行程与本行行程 x 范围相差 <=1 即相连。
        let mut i = 0usize;
        let mut j = 0usize;
        while i < current.len() && j < prev.len() {
            let cur = current[i];
            let old = prev[j];
            if old.x0 as i32 > cur.x1 as i32 + 1 {
                i += 1;
            } else if cur.x0 as i32 > old.x1 as i32 + 1 {
                j += 1;
            } else {
                union(&mut parent, cur.label, old.label);
                if old.x1 <= cur.x1 {
                    j += 1;
                } else {
                    i += 1;
                }
            }
        }

        rows.push(current.clone());
        prev = current;
    }

    let mut stats: Vec<ComponentStats> = vec![None; parent.len()];
    for (y, runs) in rows.iter().enumerate() {
        for run in runs {
            let root = find(&mut parent, run.label);
            let entry = stats[root].get_or_insert((run.x0, y as u32, run.x1, y as u32, 0));
            entry.0 = entry.0.min(run.x0);
            entry.1 = entry.1.min(y as u32);
            entry.2 = entry.2.max(run.x1);
            entry.3 = entry.3.max(y as u32);
            entry.4 += run.x1 - run.x0 + 1;
        }
    }

    stats
        .into_iter()
        .flatten()
        .map(|(x0, y0, x1, y1, area)| Comp {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
            area,
        })
        .collect()
}

/// 取花色连通域正上方的点数紧包围盒（适配王者牌面：点数在花色上方同列）。
fn rank_above(mask: &GrayImage, cx: u32, cy: u32, cw: u32, ch: u32, scale: f32) -> Option<(u32, u32, u32, u32)> {
    let (mw, mh) = mask.dimensions();
    let pad = ((4.0 * scale) as u32).max(3);
    let _gap = ((5.0 * scale) as u32).max(2);
    let x0 = cx.saturating_sub(pad);
    let x1 = (cx + cw + pad).min(mw);
    let y0 = cy.saturating_sub(ch * 2 + (8.0 * scale) as u32);
    let y1 = cy.saturating_sub(2);
    if y1 <= y0 || x1 <= x0 || y1 > mh {
        return None;
    }
    let mut minx = u32::MAX;
    let mut miny = u32::MAX;
    let mut maxx = 0u32;
    let mut maxy = 0u32;
    let mut cnt = 0u32;
    for yy in y0..y1 {
        for xx in x0..x1 {
            if mask.get_pixel(xx, yy).0[0] > 0 {
                cnt += 1;
                minx = minx.min(xx);
                maxx = maxx.max(xx);
                miny = miny.min(yy);
                maxy = maxy.max(yy);
            }
        }
    }
    if cnt < 40 || minx == u32::MAX {
        return None;
    }
    // 按行投影取最长连续字形带，自动丢弃上方孤立杂点（身份飘带/水印尖角）。
    let mut row_has: Vec<bool> = vec![false; (maxy - miny + 1) as usize];
    for yy in miny..=maxy {
        let mut any = false;
        for xx in minx..=maxx {
            if mask.get_pixel(xx, yy).0[0] > 0 {
                any = true;
                break;
            }
        }
        row_has[(yy - miny) as usize] = any;
    }
    // 找最长连续（允许 gap<=2）的 true 段
    let mut best = (0usize, 0usize, 0usize); // start,end,len
    let mut i = 0;
    while i < row_has.len() {
        if row_has[i] {
            let s0 = i;
            let mut last = i;
            let mut j = i + 1;
            while j < row_has.len() {
                if row_has[j] {
                    last = j;
                    j += 1;
                } else {
                    // 允许最多 2 行空
                    let mut k = j;
                    while k < row_has.len() && !row_has[k] && k - last <= 2 {
                        k += 1;
                    }
                    if k < row_has.len() && row_has[k] {
                        j = k + 1;
                        last = k;
                    } else {
                        break;
                    }
                }
            }
            let len = last - s0 + 1;
            if len > best.2 {
                best = (s0, last, len);
            }
            i = last + 1;
        } else {
            i += 1;
        }
    }
    if best.2 == 0 {
        return None;
    }
    let by0 = miny + best.0 as u32;
    let by1 = miny + best.1 as u32;
    // 在该行带内重新求横向范围
    let mut bx0 = u32::MAX;
    let mut bx1 = 0u32;
    for yy in by0..=by1 {
        for xx in minx..=maxx {
            if mask.get_pixel(xx, yy).0[0] > 0 {
                bx0 = bx0.min(xx);
                bx1 = bx1.max(xx);
            }
        }
    }
    if bx0 == u32::MAX {
        return None;
    }
    Some((bx0, by0, bx1 - bx0 + 1, by1 - by0 + 1))
}

#[allow(dead_code)]
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

fn normalize_rank(crop: &GrayImage, _target_h: u32) -> GrayImage {
    // 固定画布，消除不同牌宽高比带来的形变（点数模板也按 32x40 存放）。
    const RW: u32 = 32;
    const RH: u32 = 40;
    let resized = image::imageops::resize(crop, RW, RH, FilterType::Triangle);
    binarize_owned(resized)
}

fn normalize_suit(crop: &GrayImage) -> GrayImage {
    const SIZE: u32 = 40;
    let (w, h) = crop.dimensions();
    let scale = (SIZE as f32 - 6.0) / w.max(h).max(1) as f32;
    let rw = ((w as f32 * scale).round() as u32).max(1);
    let rh = ((h as f32 * scale).round() as u32).max(1);
    let filter = if rw >= w && rh >= h {
        FilterType::Nearest
    } else {
        FilterType::Triangle
    };
    let resized = image::imageops::resize(crop, rw, rh, filter);
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

/// 检测框周围“白色卡面”的占比（用于排除背景/UI/立绘误检）。
fn white_fraction(image: &RgbImage, x: u32, y: u32, w: u32, h: u32, expand: f32) -> f32 {
    let (iw, ih) = image.dimensions();
    let pad = (expand * w.max(h) as f32) as u32;
    let x0 = x.saturating_sub(pad);
    let y0 = y.saturating_sub(pad);
    let x1 = (x + w + pad).min(iw);
    let y1 = (y + h + pad).min(ih);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let sx = ((x1 - x0) / 12).max(1) as usize;
    let sy = ((y1 - y0) / 12).max(1) as usize;
    let mut sum = 0u32;
    let mut n = 0u32;
    let mut yy = y0;
    while yy < y1 {
        let mut xx = x0;
        while xx < x1 {
            let p = image.get_pixel(xx, yy).0;
            let mx = p[0].max(p[1]).max(p[2]);
            let mn = p[0].min(p[1]).min(p[2]);
            if mn > 165 && (mx - mn) < 50 {
                sum += 1;
            }
            n += 1;
            xx += sx as u32;
        }
        yy += sy as u32;
    }
    if n == 0 { 0.0 } else { sum as f32 / n as f32 }
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
            if mx > 120 && (mx as i32 - mn as i32) < 40 {
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

    // 直接在缩小图上算 CCOEFF_NORMED：比 CCORR_NORMED 更能排除暗色 UI 误匹配。
    let mut candidates = ccoeff_scan(&small_search, &small_template, threshold * 0.85);
    candidates.truncate(4);

    let mut verified = Vec::new();
    for (x, y, _coarse_score) in candidates {
        let fx = (x * factor).min(search.width().saturating_sub(template.width()));
        let fy = (y * factor).min(search.height().saturating_sub(template.height()));
        // 大多数真候选离粗定位不超过 4px；先搜小窗口，失败再扩大。
        let found = verify_candidate(search, template, fx, fy, factor, threshold)
            .or_else(|| verify_candidate(search, template, fx, fy, factor * 2, threshold));
        if let Some(hit) = found {
            verified.push(hit);
        }
    }
    verified
}

fn verify_candidate(
    search: &GrayImage,
    template: &GrayImage,
    fx: u32,
    fy: u32,
    pad: u32,
    threshold: f32,
) -> Option<(u32, u32, f32)> {
    let wx = fx.saturating_sub(pad);
    let wy = fy.saturating_sub(pad);
    let wx1 = (fx + template.width() + pad).min(search.width());
    let wy1 = (fy + template.height() + pad).min(search.height());
    if wx1 <= wx + template.width() || wy1 <= wy + template.height() {
        return None;
    }
    let window = crop(search, wx, wy, wx1 - wx, wy1 - wy);
    let (ox, oy, score) = best_ccoeff(&window, template)?;
    (score >= threshold).then_some((wx + ox, wy + oy, score))
}

/// 小图上完整扫描 CCOEFF_NORMED，返回按分数排序的局部极大值。
fn ccoeff_scan(search: &GrayImage, template: &GrayImage, threshold: f32) -> Vec<(u32, u32, f32)> {
    if template.width() > search.width() || template.height() > search.height() {
        return Vec::new();
    }
    let n = (template.width() * template.height()) as f64;
    let t_sum: f64 = template.as_raw().iter().map(|&v| v as f64).sum();
    let t_mean = t_sum / n;
    let t_var: f64 = template
        .as_raw()
        .iter()
        .map(|&v| {
            let d = v as f64 - t_mean;
            d * d
        })
        .sum();
    if t_var == 0.0 {
        return Vec::new();
    }

    let tw = template.width() as usize;
    let th = template.height() as usize;
    let sw = search.width() as usize;
    let sh = search.height() as usize;
    let raw = search.as_raw();
    let tpl = template.as_raw();
    let mut hits = Vec::new();
    for y in 0..=(sh - th) {
        for x in 0..=(sw - tw) {
            let mut sum = 0.0f64;
            let mut sum2 = 0.0f64;
            let mut cross = 0.0f64;
            for ty in 0..th {
                let row = (y + ty) * sw + x;
                for tx in 0..tw {
                    let i = raw[row + tx] as f64;
                    sum += i;
                    sum2 += i * i;
                    cross += i * tpl[ty * tw + tx] as f64;
                }
            }
            let mean = sum / n;
            let var = (sum2 - sum * mean).max(0.0);
            let cov = cross - t_mean * sum;
            let denom = (var * t_var).sqrt();
            if denom <= 1e-9 {
                continue;
            }
            let score = (cov / denom) as f32;
            if score < threshold {
                continue;
            }
            hits.push((x as u32, y as u32, score));
        }
    }
    hits.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut kept: Vec<(u32, u32, f32)> = Vec::new();
    for (x, y, score) in hits {
        if kept.iter().any(|(kx, ky, _)| {
            (*kx as i32 - x as i32).abs() <= (tw as i32 / 2).max(2)
                && (*ky as i32 - y as i32).abs() <= (th as i32 / 2).max(2)
        }) {
            continue;
        }
        kept.push((x, y, score));
    }
    kept
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

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_components(mask: &GrayImage) -> Vec<Comp> {
        let (width, height) = mask.dimensions();
        let mut seen = vec![false; (width * height) as usize];
        let mut out = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let index = (y * width + x) as usize;
                if seen[index] || mask.get_pixel(x, y).0[0] == 0 {
                    continue;
                }
                seen[index] = true;
                let mut stack = vec![(x, y)];
                let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
                let mut area = 0u32;
                while let Some((cx, cy)) = stack.pop() {
                    area += 1;
                    x0 = x0.min(cx);
                    y0 = y0.min(cy);
                    x1 = x1.max(cx);
                    y1 = y1.max(cy);
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = cx as i32 + dx;
                            let ny = cy as i32 + dy;
                            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                                continue;
                            }
                            let ni = (ny as u32 * width + nx as u32) as usize;
                            if seen[ni] || mask.get_pixel(nx as u32, ny as u32).0[0] == 0 {
                                continue;
                            }
                            seen[ni] = true;
                            stack.push((nx as u32, ny as u32));
                        }
                    }
                }
                out.push(Comp {
                    x: x0,
                    y: y0,
                    w: x1 - x0 + 1,
                    h: y1 - y0 + 1,
                    area,
                });
            }
        }
        out.sort_by_key(|c| (c.y, c.x, c.w, c.h, c.area));
        out
    }

    #[test]
    fn rle_components_match_bruteforce() {
        let mut seed = 0x1234_5678u32;
        for case in 0..80 {
            let width = 3 + (case % 17);
            let height = 3 + (case / 17);
            let mut mask = GrayImage::new(width, height);
            for y in 0..height {
                for x in 0..width {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let on = (seed >> 28) < 6;
                    mask.put_pixel(x, y, Luma([if on { 255 } else { 0 }]));
                }
            }
            let mut actual = components(&mask);
            actual.sort_by_key(|c| (c.y, c.x, c.w, c.h, c.area));
            assert_eq!(actual, brute_components(&mask), "case {case}");
        }
    }
}
