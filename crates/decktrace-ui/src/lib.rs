//! UI 抽象：TUI / GUI 都只消费 TrackerSnapshot + GameProfile。

use decktrace_core::TrackerSnapshot;
use decktrace_game_api::GameProfile;

pub trait Renderer {
    fn start(&mut self) -> Result<(), String>;
    fn update(&mut self, snapshot: &TrackerSnapshot, game: &GameProfile, status: &str);
    fn close(&mut self);
    fn is_open(&self) -> bool;
}
