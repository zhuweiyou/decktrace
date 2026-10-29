use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tracking::{BottomConfig, GameProfile};
use tracking::{JokerBand, RecognitionConfig, ResetConfig, WindowSelector};

/// 资源根目录：优先环境变量，其次 .app 内的 Contents/Resources，最后开发目录。
fn asset_root() -> PathBuf {
    if let Ok(root) = std::env::var("DECKTRACE_ASSETS") {
        let path = PathBuf::from(root);
        if path.join("wzddz/templates").exists() {
            return path.join("wzddz");
        }
        return path;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents) = exe.parent().and_then(|macos| macos.parent()) {
            let bundled = contents.join("Resources/assets/wzddz");
            if bundled.join("templates").exists() {
                return bundled;
            }
        }
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("assets/wzddz");
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

    // 尺寸按 900×550 窗口、2x 截图（1800×1100）实测：
    // 底牌约 41×80，位于顶部中间；手牌 85×170，横排于底部。
    let recognition = RecognitionConfig {
        rank_template_height: 70,
        reference_width: 1800,
        joker_template: "joker.png".to_string(),
        joker_compact_template: Some("joker_compact.png".to_string()),
        joker_bands: vec![
            // 顶部底牌栏（记牌器收起时位于中间偏右）
            JokerBand {
                top_ratio: 0.00,
                bottom_ratio: 0.22,
                template: "joker_compact.png".to_string(),
                scales: vec![0.90, 1.00, 1.10],
            },
            // 三家出牌区
            JokerBand {
                top_ratio: 0.22,
                bottom_ratio: 0.58,
                template: "joker.png".to_string(),
                scales: vec![0.54, 0.60, 0.66, 0.72],
            },
            // 手牌区
            JokerBand {
                top_ratio: 0.60,
                bottom_ratio: 1.00,
                template: "joker.png".to_string(),
                scales: vec![0.92, 1.00, 1.08],
            },
        ],
        rank_min_score: 0.45,
        suit_min_score: 0.45,
        rank_margin: 0.03,
        face_min_fraction: 0.18,
        ..RecognitionConfig::default()
    };

    // 手机横屏小程序手牌：字形底部约在 0.776（2780×1264 实测）。
    // hand_bottom_ratio 必须小于字形底部，否则自动重置判定不到手牌。
    let reset = ResetConfig {
        hand_top_ratio: 0.68,
        hand_bottom_ratio: 0.85,
        min_hand_cards: 13,
        min_hand_for_jump: 17,
        empty_hand_threshold: 5,
        seen_for_reset: 15,
        overlap_threshold: 0.60,
        absent_seconds: 1.5,
        min_reset_gap: 2.0,
    };

    GameProfile {
        id: "wzddz".to_string(),
        name: "王者斗地主".to_string(),
        window: WindowSelector::new("王者荣耀"),
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
        // 手机横屏小程序三张底牌在牌桌顶部正中：中心 y≈0.07，x≈0.47–0.51。
        // 区域取略大的搜索带；记牌器面板/礼包图标没有点数花色，会被识别阈值过滤。
        bottom: Some(Arc::new(BottomConfig::new(
            (0.42, 0.02, 0.70, 0.19),
            3,
            2,
        ))),
    }
}
