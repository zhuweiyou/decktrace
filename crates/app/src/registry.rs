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
        "wzddz" => {
            let game = wzddz::profile();
            game.validate()?;
            Ok(game)
        }
        other => Err(format!(
            "未知游戏：{other}，可用：{}",
            GAME_IDS.join(", ")
        )),
    }
}

/// 所有可用 game id（含手机版；`--game` 与错误提示用）。
pub const GAME_IDS: &[&str] = &["jjddz", "hlddz", "hlddz_mobile", "wzddz"];

/// 手机版游戏：没有 PC 窗口，只能走 `--capture adb` 调试或 APK/JNI，不参与 PC 窗口自动检测。
pub fn is_mobile_game(id: &str) -> bool {
    matches!(id, "hlddz_mobile" | "wzddz")
}

/// PC 端“窗口自动检测”的游戏列表。**只含 PC 游戏**；手机版不在 PC 上找窗口。
pub fn list_games() -> Vec<GameProfile> {
    vec![jjddz::profile(), hlddz::profile()]
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
        "wzddz" => {
            let recognizer =
                wzddz::WzddzRecognizer::new(game).map_err(|err| err.to_string())?;
            Ok(Arc::new(recognizer))
        }
        other => Err(format!("没有为游戏 {other} 注册识别器")),
    }
}
