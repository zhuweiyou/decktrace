use std::fmt;

pub type BBox = (u32, u32, u32, u32);

pub const SUIT_SYMBOLS: &[(&str, &str)] = &[("H", "♥"), ("D", "♦"), ("S", "♠"), ("C", "♣")];

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowSelector {
    pub title: String,
    pub app: Option<String>,
    pub window_id: Option<u64>,
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
