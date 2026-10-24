//! 游戏注册表。新增游戏时：新建 `decktrace-game-xxx` crate，然后在这里注册一行。

use std::sync::OnceLock;

use decktrace_game_api::GameProfile;

fn games() -> &'static [GameProfile] {
    static GAMES: OnceLock<Vec<GameProfile>> = OnceLock::new();
    GAMES.get_or_init(|| vec![decktrace_game_jj::profile()])
}

pub fn get_game(id: &str) -> Result<&'static GameProfile, String> {
    games()
        .iter()
        .find(|game| game.id == id)
        .ok_or_else(|| format!("未知游戏：{id}，可用：{}", game_ids().join(", ")))
}

pub fn list_games() -> &'static [GameProfile] {
    games()
}

pub fn game_ids() -> Vec<&'static str> {
    games().iter().map(|game| game.id.as_str()).collect()
}
