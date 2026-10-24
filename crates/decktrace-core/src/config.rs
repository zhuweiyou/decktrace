use serde::{Deserialize, Serialize};

/// 大小王模板搜索的一条高度带。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JokerBand {
    pub top_ratio: f32,
    pub bottom_ratio: f32,
    pub template: String,
    pub scales: Vec<f32>,
}

/// 模板识别的通用参数；具体数值由游戏 profile 提供。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognitionConfig {
    pub rank_template_height: u32,
    pub ref_suit_height: f32,
    pub reference_width: u32,
    pub reference_height: u32,
    pub rank_min_score: f32,
    pub suit_min_score: f32,
    pub face_min_fraction: f32,
    pub rank_margin: f32,
    pub joker_template: String,
    pub joker_compact_template: Option<String>,
    pub joker_bands: Vec<JokerBand>,
    /// 大小王模板匹配的最低分数。
    pub joker_min_score: f32,
}

impl Default for RecognitionConfig {
    fn default() -> Self {
        Self {
            rank_template_height: 70,
            ref_suit_height: 45.0,
            reference_width: 1800,
            reference_height: 1100,
            rank_min_score: 0.55,
            suit_min_score: 0.60,
            face_min_fraction: 0.25,
            rank_margin: 0.08,
            joker_template: "joker.png".to_string(),
            joker_compact_template: Some("joker_compact.png".to_string()),
            joker_bands: Vec::new(),
            joker_min_score: 0.62,
        }
    }
}

/// 自动重置规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetConfig {
    pub hand_top_ratio: f32,
    pub hand_bottom_ratio: f32,
    pub min_hand_cards: usize,
    pub min_hand_for_jump: usize,
    pub empty_hand_threshold: usize,
    pub seen_for_reset: usize,
    pub overlap_threshold: f32,
    pub absent_seconds: f64,
    pub min_reset_gap: f64,
}

impl Default for ResetConfig {
    fn default() -> Self {
        Self {
            hand_top_ratio: 0.60,
            hand_bottom_ratio: 0.70,
            min_hand_cards: 12,
            min_hand_for_jump: 15,
            empty_hand_threshold: 5,
            seen_for_reset: 15,
            overlap_threshold: 0.60,
            absent_seconds: 1.5,
            min_reset_gap: 2.0,
        }
    }
}
