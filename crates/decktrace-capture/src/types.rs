use std::fmt;

use image::RgbImage;

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

#[derive(Clone)]
pub struct RawFrame {
    pub seq: u64,
    pub timestamp: f64,
    pub image: RgbImage,
    pub width: u32,
    pub height: u32,
}
