use std::sync::Arc;

use tracking::{GameProfile, SharedRecognizer};

/// 游戏注册表。新增游戏时在这里加一个分支，并新建 `crates/<id>` 游戏 crate（目录名 == GameProfile.id）。
pub fn get_game(id: &str) -> Result<GameProfile, String> {
    match id {
        "jjddz" => {
            let game = jjddz::profile();
            game.validate()?;
            Ok(game)
        }
        "hlddz" => {
            let game = hlddz::profile();
            game.validate()?;
            Ok(game)
        }
        "hlddz_mobile" => {
            let game = hlddz_mobile::profile();
            game.validate()?;
            Ok(game)
        }
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
    vec![jjddz::profile(), hlddz::profile(), hlddz_mobile::profile()]
}

/// 为游戏创建识别器。新增游戏时在这里加一个分支。
pub fn create_recognizer(game: &GameProfile) -> Result<SharedRecognizer, String> {
    match game.id.as_str() {
        "jjddz" => {
            let recognizer = jjddz::JjddzRecognizer::new(game).map_err(|err| err.to_string())?;
            Ok(Arc::new(recognizer))
        }
        "hlddz" => {
            let recognizer = hlddz::HlddzRecognizer::new(game).map_err(|err| err.to_string())?;
            Ok(Arc::new(recognizer))
        }
        "hlddz_mobile" => {
            let recognizer =
                hlddz_mobile::HlddzRecognizer::new(game).map_err(|err| err.to_string())?;
            Ok(Arc::new(recognizer))
        }
        other => Err(format!("没有为游戏 {other} 注册识别器")),
    }
}
