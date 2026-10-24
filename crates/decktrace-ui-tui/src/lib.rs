//! 终端渲染：保留 fps/队列/识别耗时等调试信息。

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use decktrace_core::TrackerSnapshot;
use decktrace_game_api::GameProfile;
use decktrace_ui::Renderer;
use unicode_width::UnicodeWidthStr;

pub struct TuiRenderer {
    color: bool,
    open: bool,
    interactive: bool,
    last_print: Option<Instant>,
}

impl TuiRenderer {
    pub fn new(color: bool) -> Self {
        Self {
            color,
            open: false,
            interactive: io::stdout().is_terminal(),
            last_print: None,
        }
    }
}

impl Renderer for TuiRenderer {
    fn start(&mut self) -> Result<(), String> {
        if self.interactive {
            enable_raw_mode().map_err(|err| err.to_string())?;
            execute!(
                io::stdout(),
                EnterAlternateScreen,
                Hide,
                Clear(ClearType::All)
            )
            .map_err(|err| err.to_string())?;
        }
        self.open = true;
        Ok(())
    }

    fn update(&mut self, snapshot: &TrackerSnapshot, game: &GameProfile, status: &str) {
        if !self.open {
            return;
        }
        if !self.interactive {
            // 非 TTY（e2e/管道）下降低刷新频率，避免刷屏。
            let now = Instant::now();
            if self
                .last_print
                .is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
            {
                return;
            }
            self.last_print = Some(now);
            print!("{}", render_text(snapshot, game, status, self.color));
            let _ = io::stdout().flush();
            return;
        }
        let text = render_text(snapshot, game, status, self.color);
        let mut out = io::stdout();
        let _ = execute!(out, MoveTo(0, 0), Clear(ClearType::All));
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }

    fn close(&mut self) {
        if self.open {
            if self.interactive {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
            }
            self.open = false;
        }
    }

    fn is_open(&self) -> bool {
        self.open
    }
}

impl Drop for TuiRenderer {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn render_text(
    snapshot: &TrackerSnapshot,
    game: &GameProfile,
    status: &str,
    color: bool,
) -> String {
    let mut out = String::new();
    let summary = if snapshot.armed {
        format!(
            "已识别 {}/{}   剩余 {} 张",
            snapshot.seen_count,
            game.total_cards(),
            snapshot.remaining
        )
    } else {
        "等待底牌…（识别到三张底牌后开始记牌）".to_string()
    };
    out.push_str(&format!("斗地主记牌器    {summary}\n\n"));

    let headers: Vec<String> = game
        .display_ranks
        .iter()
        .map(|rank| game.label(rank, ""))
        .collect();
    let plain_cells: Vec<String> = game
        .display_ranks
        .iter()
        .map(|rank| {
            let value = snapshot.counts.get(rank).copied().unwrap_or(0);
            if value == 0 {
                String::new()
            } else {
                value.to_string()
            }
        })
        .collect();
    let colored_cells: Vec<String> = if color {
        plain_cells
            .iter()
            .map(|cell| {
                if cell.is_empty() {
                    String::new()
                } else {
                    format!("\x1b[1;34m{cell}\x1b[0m")
                }
            })
            .collect()
    } else {
        plain_cells.clone()
    };
    out.push_str(&format_table(&headers, &plain_cells, &colored_cells));

    if !snapshot.bottom_ids.is_empty() {
        let labels: Vec<String> = game
            .all_card_ids()
            .into_iter()
            .filter(|id| snapshot.bottom_ids.contains(id))
            .map(|id| label_of_id(&id, game))
            .collect();
        out.push_str(&format!(
            "底牌（未出，仍计入剩余）：{}\n",
            labels.join("  ")
        ));
    }

    let recent: Vec<String> = snapshot
        .recent
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|det| det.label())
        .collect();
    out.push_str(&format!(
        "最近入账：{}\n",
        if recent.is_empty() {
            "等待识别到牌…".to_string()
        } else {
            recent.join("  ")
        }
    ));
    if !status.is_empty() {
        out.push_str(&format!("{status}\n"));
    }
    out.push_str("颜色越深 = 剩余越多；0 不显示    r 重置本局    q 退出\n");
    out
}

fn label_of_id(id: &str, game: &GameProfile) -> String {
    if game.labels.contains_key(id) {
        return game.label(id, "");
    }
    let mut chars = id.chars();
    let suit = chars.next_back().unwrap_or_default().to_string();
    let rank: String = chars.collect();
    game.label(&rank, &suit)
}

fn format_table(headers: &[String], plain: &[String], colored: &[String]) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .zip(plain)
        .map(|(header, cell)| header.width().max(cell.width()).max(2))
        .collect();
    let mut out = String::new();
    for (header, width) in headers.iter().zip(&widths) {
        out.push_str(&pad(header, *width));
        out.push(' ');
    }
    out.push('\n');
    for width in &widths {
        out.push_str(&"-".repeat(*width));
        out.push(' ');
    }
    out.push('\n');
    for (index, width) in widths.iter().enumerate() {
        let cell = &colored[index];
        if cell.contains("\x1b[") {
            out.push_str(cell);
            out.push_str(&" ".repeat(width.saturating_sub(plain[index].width())));
        } else {
            out.push_str(&pad(cell, *width));
        }
        out.push(' ');
    }
    out.push('\n');
    out
}

fn pad(text: &str, width: usize) -> String {
    let current = text.width();
    format!("{text}{}", " ".repeat(width.saturating_sub(current)))
}
