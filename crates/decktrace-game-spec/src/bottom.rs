use image::RgbImage;

use decktrace_core::Detection;

/// 底牌识别策略。
///
/// 不同游戏的底牌位置、张数、识别方式都不同，甚至可能没有底牌。
/// 通用识别引擎只调用 [`BottomSpec::mark`]；记牌状态机只读取
/// [`BottomSpec::count`] / [`BottomSpec::stable_frames`] 做开局门控。
pub trait BottomSpec: Send + Sync {
    fn count(&self) -> usize;
    fn stable_frames(&self) -> usize;
    fn mark(&self, image: &RgbImage, detections: Vec<Detection>) -> Vec<Detection>;
}

/// 区域型底牌识别：归一化矩形内、连续多帧稳定出现才算底牌。
#[derive(Debug, Clone, Copy)]
pub struct BottomConfig {
    /// 归一化坐标 (left, top, right, bottom)。
    pub region: (f32, f32, f32, f32),
    pub count: usize,
    pub stable_frames: usize,
}

impl BottomConfig {
    pub const fn new(region: (f32, f32, f32, f32), count: usize, stable_frames: usize) -> Self {
        Self {
            region,
            count,
            stable_frames,
        }
    }
}

impl BottomSpec for BottomConfig {
    fn count(&self) -> usize {
        self.count
    }

    fn stable_frames(&self) -> usize {
        self.stable_frames
    }

    fn mark(&self, image: &RgbImage, detections: Vec<Detection>) -> Vec<Detection> {
        let (width, height) = image.dimensions();
        let (x0, y0, x1, y1) = self.region;
        detections
            .into_iter()
            .map(|mut det| {
                let (_, _, w, h) = det.bbox;
                let cx = (det.bbox.0 as f32 + w as f32 / 2.0) / width.max(1) as f32;
                let cy = (det.bbox.1 as f32 + h as f32 / 2.0) / height.max(1) as f32;
                if x0 <= cx && cx <= x1 && y0 <= cy && cy <= y1 {
                    det.is_bottom = true;
                }
                det
            })
            .collect()
    }
}
