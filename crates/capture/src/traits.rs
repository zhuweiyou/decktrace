//! 平台与采集抽象。

use std::time::Duration;

use tracking::WindowSelector;

use crate::types::{RawFrame, WindowInfo};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("窗口错误：{0}")]
    Window(String),
    #[error("采集错误：{0}")]
    Capture(String),
    #[error("后端尚未实现：{0}")]
    Unsupported(String),
    #[error("平台错误：{0}")]
    Platform(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

pub trait WindowService: Send + Sync {
    fn list_windows(&self, include_off_screen: bool) -> Result<Vec<WindowInfo>>;

    fn find_window(&self, selector: &WindowSelector) -> Result<WindowInfo> {
        select_window(&self.list_windows(selector.include_off_screen)?, selector)
    }
}

pub trait CaptureStream: Send {
    fn start(&mut self) -> Result<()>;
    fn read(&mut self, timeout: Duration) -> Result<Option<RawFrame>>;
    fn stop(&mut self);
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &str;
    fn window_service(&self) -> &dyn WindowService;
    fn create_capture_stream(
        &self,
        window: &WindowInfo,
        fps: f32,
    ) -> Result<Box<dyn CaptureStream>>;
}

/// 与 Python 版一致的窗口选择：ID 优先，其次标题/应用名打分。
pub fn select_window(windows: &[WindowInfo], selector: &WindowSelector) -> Result<WindowInfo> {
    if let Some(id) = selector.window_id {
        return windows
            .iter()
            .find(|window| window.id == id)
            .cloned()
            .ok_or_else(|| CaptureError::Window(format!("找不到窗口 ID {id}")));
    }

    let title_key = selector.title.trim().to_lowercase();
    let app_key = selector
        .app
        .as_deref()
        .map(|app| app.trim().to_lowercase())
        .filter(|app| !app.is_empty());

    let mut scored: Vec<(f64, f64, WindowInfo)> = Vec::new();
    for window in windows {
        if window.width < selector.min_width || window.height < selector.min_height {
            continue;
        }
        if window.layer != 0 {
            continue;
        }
        let window_title = window.title.trim().to_lowercase();
        let window_app = window.app.trim().to_lowercase();
        if let Some(app) = &app_key
            && !window_app.contains(app)
        {
            continue;
        }
        let score = if title_key.is_empty() {
            50.0
        } else if window_title == title_key {
            100.0
        } else if window_title.starts_with(&title_key) {
            80.0
        } else if window_title.contains(&title_key) {
            60.0
        } else if !window_title.is_empty() && title_key.contains(&window_title) {
            30.0
        } else {
            continue;
        };
        let mut score = score;
        if let Some(app) = &app_key
            && &window_app == app
        {
            score += 15.0;
        }
        if window.onscreen {
            score += 5.0;
        }
        score += (window.width * window.height / 1_000_000.0).min(5.0);
        scored.push((score, window.width * window.height, window.clone()));
    }

    if scored.is_empty() {
        return Err(no_window_error(
            windows,
            selector,
            &title_key,
            app_key.as_deref(),
        ));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.total_cmp(&a.1)));
    Ok(scored.remove(0).2)
}

fn no_window_error(
    windows: &[WindowInfo],
    selector: &WindowSelector,
    title_key: &str,
    app_key: Option<&str>,
) -> CaptureError {
    let mut candidates: Vec<&WindowInfo> = windows
        .iter()
        .filter(|w| {
            w.layer == 0 && w.width >= selector.min_width && w.height >= selector.min_height
        })
        .collect();
    candidates.sort_by_key(|w| {
        (
            w.title.to_lowercase().contains(title_key),
            app_key.is_some_and(|app| w.app.to_lowercase().contains(app)),
            (w.width * w.height) as i64,
        )
    });
    candidates.reverse();
    let body = candidates
        .iter()
        .take(12)
        .map(|w| format!("  #{}  {}  {:?}  {}", w.id, w.app, w.title, w.size()))
        .collect::<Vec<_>>()
        .join("\n");
    CaptureError::Window(format!(
        "没有找到标题匹配 {:?} 的窗口。\n可用窗口：\n{}\n提示：运行 `list` 查看完整窗口列表。",
        selector.title,
        if body.is_empty() {
            "  （无）"
        } else {
            &body
        }
    ))
}
