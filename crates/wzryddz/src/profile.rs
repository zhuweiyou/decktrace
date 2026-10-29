use std::collections::BTreeMap;
use std::path::PathBuf;

use tracking::{GameProfile, RecognitionConfig, ResetConfig, WindowSelector};

/// 资源根目录：优先环境变量，其次 .app 内的 Contents/Resources，最后开发目录。
fn asset_root() -> PathBuf {
    if let Ok(root) = std::env::var("DECKTRACE_ASSETS") {
        let path = PathBuf::from(root);
        if path.join("wzryddz/templates").exists() {
            return path.join("wzryddz");
        }
        return path;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents) = exe.parent().and_then(|macos| macos.parent()) {
            let bundled = contents.join("Resources/assets/wzryddz");
            if bundled.join("templates").exists() {
                return bundled;
            }
        }
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("assets/wzryddz");
            if adjacent.join("templates").exists() {
                return adjacent;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

pub fn profile() -> GameProfile {
    build(asset_root())
}

/// 用指定资源根目录构建 profile（Android/JNI 用：assets 解压到 filesDir 后传入）。
pub fn profile_with_assets(assets: PathBuf) -> GameProfile {
    build(assets)
}

fn build(assets: PathBuf) -> GameProfile {
    let mut rank_totals = BTreeMap::new();
    for (rank, total) in [
        ("BJ", 1u8),
        ("SJ", 1),
        ("2", 4),
        ("A", 4),
        ("K", 4),
        ("Q", 4),
        ("J", 4),
        ("10", 4),
        ("9", 4),
        ("8", 4),
        ("7", 4),
        ("6", 4),
        ("5", 4),
        ("4", 4),
        ("3", 4),
    ] {
        rank_totals.insert(rank.to_string(), total);
    }

    let mut labels = BTreeMap::new();
    labels.insert("BJ".to_string(), "大王".to_string());
    labels.insert("SJ".to_string(), "小王".to_string());

    let display_ranks = [
        "BJ", "SJ", "2", "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    // 识别参数暂用默认值；等真实素材/区域标定后再覆盖。
    GameProfile {
        id: "wzryddz".to_string(),
        name: "王者荣耀斗地主".to_string(),
        window: WindowSelector::new("王者荣耀"),
        templates_dir: assets.join("templates"),
        rank_totals,
        display_ranks,
        labels,
        suits: ["H", "D", "S", "C"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        recognition: RecognitionConfig::default(),
        reset: ResetConfig::default(),
        // 占位阶段不做底牌门控。
        bottom: None,
    }
}
