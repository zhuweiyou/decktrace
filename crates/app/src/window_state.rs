//! 记住窗口位置，避免每次启动都回到默认位置。

use std::fs;
use std::path::PathBuf;

use eframe::egui;

pub fn load() -> Option<egui::Pos2> {
    let text = fs::read_to_string(path()?).ok()?;
    parse(&text)
}

pub fn save(pos: egui::Pos2) {
    let Some(path) = path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, format!("{:.1},{:.1}", pos.x, pos.y));
}

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support/decktrace")
            .join("window-position.txt"),
    )
}

fn parse(text: &str) -> Option<egui::Pos2> {
    let (x, y) = text.trim().split_once(',')?;
    let x: f32 = x.trim().parse().ok()?;
    let y: f32 = y.trim().parse().ok()?;
    (x.is_finite() && y.is_finite()).then_some(egui::pos2(x, y))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_position() {
        let pos = parse("120.5, 340").expect("position");
        assert_eq!(pos.x, 120.5);
        assert_eq!(pos.y, 340.0);
        assert!(parse("bad").is_none());
    }
}
