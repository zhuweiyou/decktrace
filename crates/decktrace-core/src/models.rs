use std::fmt;

use image::RgbImage;
use serde::{Deserialize, Serialize};

pub type BBox = (u32, u32, u32, u32);

pub const SUIT_SYMBOLS: &[(&str, &str)] = &[("H", "♥"), ("D", "♦"), ("S", "♠"), ("C", "♣")];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub rank: String,
    pub suit: String,
    pub score: f32,
    pub bbox: BBox,
    pub rank_score: f32,
    pub suit_score: f32,
    pub face_score: f32,
    pub display: String,
    pub is_bottom: bool,
}

impl Detection {
    pub fn card_id(&self) -> String {
        if self.suit.is_empty() {
            self.rank.clone()
        } else {
            format!("{}{}", self.rank, self.suit)
        }
    }

    pub fn label(&self) -> String {
        if !self.display.is_empty() {
            return self.display.clone();
        }
        if let Some((_, symbol)) = SUIT_SYMBOLS.iter().find(|(s, _)| *s == self.suit) {
            format!("{}{}", self.rank, symbol)
        } else {
            self.rank.clone()
        }
    }
}

impl fmt::Display for Detection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{:.2}]", self.label(), self.score)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app: String,
    pub pid: i32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub onscreen: bool,
    pub layer: i32,
    pub scale: f64,
}

impl WindowInfo {
    pub fn size(&self) -> String {
        format!("{:.0}x{:.0}", self.width, self.height)
    }

    pub fn position(&self) -> String {
        format!("({:.0}, {:.0})", self.x, self.y)
    }
}

impl fmt::Display for WindowInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{} {} / {:?} {} @ {}",
            self.id,
            self.app,
            self.title,
            self.size(),
            self.position()
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowSelector {
    pub title: String,
    pub app: Option<String>,
    pub window_id: Option<u32>,
    pub include_off_screen: bool,
    pub min_width: f64,
    pub min_height: f64,
}

impl WindowSelector {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            min_width: 120.0,
            min_height: 120.0,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct RawFrame {
    pub seq: u64,
    pub timestamp: f64,
    pub image: RgbImage,
    pub width: u32,
    pub height: u32,
}
