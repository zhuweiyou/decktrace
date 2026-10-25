use std::collections::BTreeMap;
use std::sync::Arc;

use tracking::{CardTracker, Detection, GameProfile, WindowSelector};

fn game() -> Arc<GameProfile> {
    let mut rank_totals = BTreeMap::new();
    for rank in ["2", "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3"] {
        rank_totals.insert(rank.to_string(), 4u8);
    }
    Arc::new(GameProfile {
        id: "test".into(),
        name: "test".into(),
        window: WindowSelector::new("test"),
        templates_dir: Default::default(),
        rank_totals,
        display_ranks: ["2", "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        labels: BTreeMap::new(),
        suits: ["H", "D", "S", "C"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        recognition: Default::default(),
        reset: Default::default(),
        bottom: None,
    })
}

fn card(score: f32) -> Detection {
    Detection {
        rank: "8".into(),
        suit: "H".into(),
        score,
        bbox: (100, 400, 20, 20),
        rank_score: 0.67,
        suit_score: 0.88,
        face_score: 0.8,
        display: "8♥".into(),
        is_bottom: false,
    }
}

#[test]
fn pending_candidate_needs_second_frame() {
    // 第一次看到牌（分数低于即时阈值）要挂起等待第二帧；
    // GUI 层根据 pending_count 决定是否允许跳过静止帧。
    let mut tracker = CardTracker::new(game(), 2, 0.92, 1.6);
    tracker.observe(std::slice::from_ref(&card(0.73)), 0.0);
    assert_eq!(tracker.pending_count(), 1, "单帧命中应挂起等待确认");
    tracker.observe(std::slice::from_ref(&card(0.73)), 0.1);
    assert_eq!(tracker.pending_count(), 0, "第二帧应确认入账");
    assert!(tracker.seen_ids().contains("8H"));
}

#[test]
fn instant_score_commits_in_one_frame() {
    let mut tracker = CardTracker::new(game(), 2, 0.92, 1.6);
    tracker.observe(std::slice::from_ref(&card(0.95)), 0.0);
    assert_eq!(tracker.pending_count(), 0);
    assert!(tracker.seen_ids().contains("8H"));
}
