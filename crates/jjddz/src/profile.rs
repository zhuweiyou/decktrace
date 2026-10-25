use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tracking::{BottomConfig, GameProfile};
use tracking::{JokerBand, RecognitionConfig, ResetConfig, WindowSelector};

/// 资源根目录：优先环境变量，其次 .app 内的 Contents/Resources，最后开发目录。
fn asset_root() -> PathBuf {
    if let Ok(root) = std::env::var("DECKTRACE_ASSETS") {
        let path = PathBuf::from(root);
        if path.join("jjddz/templates").exists() {
            return path.join("jjddz");
        }
        return path;
    }
    if let Ok(exe) = std::env::current_exe() {
        // decktrace.app/Contents/MacOS/decktrace -> Contents/Resources/assets/jjddz
        if let Some(contents) = exe.parent().and_then(|macos| macos.parent()) {
            let bundled = contents.join("Resources/assets/jjddz");
            if bundled.join("templates").exists() {
                return bundled;
            }
        }
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("assets/jjddz");
            if adjacent.join("templates").exists() {
                return adjacent;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

pub fn profile() -> GameProfile {
    let assets = asset_root();
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

    let recognition = RecognitionConfig {
        joker_template: "joker.png".to_string(),
        joker_compact_template: Some("joker_compact.png".to_string()),
        joker_bands: vec![
            // 底牌栏：压缩 JOK 字形
            JokerBand {
                top_ratio: 0.00,
                bottom_ratio: 0.22,
                template: "joker_compact.png".to_string(),
                scales: vec![0.90, 1.00, 1.10],
            },
            // 出牌区：约为手牌尺寸的 0.6 倍
            JokerBand {
                top_ratio: 0.22,
                bottom_ratio: 0.50,
                template: "joker.png".to_string(),
                scales: vec![0.54, 0.60, 0.66, 0.72],
            },
            // 手牌区
            JokerBand {
                top_ratio: 0.62,
                bottom_ratio: 1.00,
                template: "joker.png".to_string(),
                scales: vec![0.92, 1.00, 1.08],
            },
        ],
        ..RecognitionConfig::default()
    };

    let reset = ResetConfig {
        hand_top_ratio: 0.60,
        hand_bottom_ratio: 0.70,
        min_hand_cards: 12,
        min_hand_for_jump: 15,
        empty_hand_threshold: 5,
        seen_for_reset: 15,
        overlap_threshold: 0.60,
        absent_seconds: 1.5,
        min_reset_gap: 2.0,
    };

    GameProfile {
        id: "jjddz".to_string(),
        name: "JJ斗地主".to_string(),
        window: WindowSelector::new("JJ斗地主"),
        templates_dir: assets.join("templates"),
        rank_totals,
        display_ranks,
        labels,
        suits: ["H", "D", "S", "C"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        recognition,
        reset,
        // 三张底牌固定在顶部偏左；其他游戏可以给不同区域或自定义 BottomSpec。
        bottom: Some(Arc::new(BottomConfig::new((0.33, 0.08, 0.47, 0.20), 3, 2))),
    }
}
