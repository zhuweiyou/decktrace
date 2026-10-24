use std::sync::Arc;

use image::RgbImage;

use crate::Detection;

#[derive(Debug, Clone)]
pub struct RecognitionResult {
    pub detections: Vec<Detection>,
    pub elapsed_ms: f64,
    pub frame_width: u32,
    pub frame_height: u32,
}

/// 识别抽象：游戏实现负责具体模板/阈值，运行时只依赖这个 trait。
pub trait Recognizer: Send + Sync {
    fn recognize(&self, image: &RgbImage, detect_jokers: bool) -> RecognitionResult;
}

pub type SharedRecognizer = Arc<dyn Recognizer>;
