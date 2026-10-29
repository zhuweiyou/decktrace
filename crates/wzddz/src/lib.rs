//! 王者斗地主：**基础架子**。
//!
//! 目前只有 `GameProfile` 与一个占位识别器（不识别任何牌，返回空结果）。
//! 识别实现与模板/素材待后续补齐；补齐后把 `WzddzRecognizer::recognize`
//! 换成真实实现即可，接口不变。

pub mod profile;
pub mod recognition;

pub use profile::{profile, profile_with_assets};
pub use recognition::{RecognitionError, WzddzRecognizer};

use image::RgbImage;
use imageproc::template_matching::{MatchTemplateMethod, match_template};

const SETTLE_EXIT: &[u8] = include_bytes!("../assets/settle_exit.png");
const SETTLE_CONTINUE: &[u8] = include_bytes!("../assets/settle_continue.png");

/// 结算界面检测：返回 (exit_score, continue_score)，越小越像（SSE 归一化）。
pub fn settlement_scores(image: &RgbImage) -> (f32, f32) {
    let (w, h) = image.dimensions();
    if w < 200 || h < 100 {
        return (9.0, 9.0);
    }
    let x0 = (w as f32 * 0.55) as u32;
    let y0 = (h as f32 * 0.78) as u32;
    let x1 = (w as f32 * 0.99) as u32;
    let y1 = (h as f32 * 0.99) as u32;
    if x1 <= x0 + 30 || y1 <= y0 + 20 {
        return (9.0, 9.0);
    }
    let region_rgb = image::imageops::crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image();
    let region = image::DynamicImage::ImageRgb8(region_rgb).to_luma8();
    let sw = ((x1 - x0) / 3).max(1);
    let sh = ((y1 - y0) / 3).max(1);
    let small = image::imageops::resize(&region, sw, sh, image::imageops::FilterType::Nearest);
    let scale = (w as f32 / 2780.0).max(0.15);
    let mut best = [9.0f32, 9.0f32];
    for (i, bytes) in [SETTLE_EXIT, SETTLE_CONTINUE].into_iter().enumerate() {
        let Ok(img) = image::load_from_memory(bytes) else {
            continue;
        };
        let t = img.to_luma8();
        let tw = ((t.width() as f32 * scale / 3.0).round() as u32).max(6);
        let th = ((t.height() as f32 * scale / 3.0).round() as u32).max(6);
        if tw >= sw || th >= sh {
            continue;
        }
        let ts = image::imageops::resize(&t, tw, th, image::imageops::FilterType::Triangle);
        let m = match_template(&small, &ts, MatchTemplateMethod::SumOfSquaredErrorsNormalized);
        for p in m.pixels() {
            let v = p[0];
            if v.is_finite() && v < best[i] {
                best[i] = v;
            }
        }
    }
    (best[0], best[1])
}

/// 是否处于结算界面（退出/继续游戏按钮出现）。阈值经样本标定。
pub fn detect_settlement(image: &RgbImage) -> bool {
    let (e, c) = settlement_scores(image);
    // 至少一个按钮高度相似（SSE 归一化很小）
    e < 0.06 || c < 0.06
}
