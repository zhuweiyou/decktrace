use image::RgbImage;
use thiserror::Error;

use tracking::{GameProfile, RecognitionResult, Recognizer};

#[derive(Debug, Error)]
pub enum RecognitionError {
    #[error("王者荣耀斗地主识别暂未实现")]
    NotImplemented,
}

/// 占位识别器：暂不识别任何牌，返回空结果。
///
/// 目的是让端上整条链路（采集 → JNI → 记牌状态机 → 悬浮窗）能先跑通；
/// 真实识别与模板就绪后替换 `recognize` 的实现即可。
pub struct WzryddzRecognizer;

impl WzryddzRecognizer {
    pub fn new(_game: &GameProfile) -> Result<Self, RecognitionError> {
        Ok(Self)
    }
}

impl Recognizer for WzryddzRecognizer {
    fn recognize(&self, image: &RgbImage, _detect_jokers: bool) -> RecognitionResult {
        RecognitionResult {
            detections: Vec::new(),
            elapsed_ms: 0.0,
            frame_width: image.width(),
            frame_height: image.height(),
        }
    }
}
