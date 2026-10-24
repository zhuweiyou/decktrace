use decktrace_tracking::GameProfile;

/// 游戏注册表。新增游戏时在这里加一个分支，并新建 `decktrace-game-xxx` crate。
pub fn get_game(id: &str) -> Result<GameProfile, String> {
    match id {
        "jj" => Ok(decktrace_jj::profile()),
        other => Err(format!(
            "未知游戏：{other}，可用：{}",
            list_games()
                .iter()
                .map(|game| game.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn list_games() -> Vec<GameProfile> {
    vec![decktrace_jj::profile()]
}
