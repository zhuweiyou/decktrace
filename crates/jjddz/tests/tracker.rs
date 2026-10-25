use std::sync::Arc;

use jjddz::profile;
use tracking::{CardTracker, Detection, GameProfile};

fn game() -> GameProfile {
    profile()
}

fn det(rank: &str, suit: &str, bottom: bool) -> Detection {
    Detection {
        rank: rank.to_string(),
        suit: suit.to_string(),
        score: 0.95,
        bbox: (0, 0, 10, 10),
        rank_score: 0.95,
        suit_score: 0.95,
        face_score: 1.0,
        display: String::new(),
        is_bottom: bottom,
    }
}

macro_rules! bottom {
    ($(($rank:expr, $suit:expr)),* $(,)?) => {
        vec![$( {
            let mut detection = det($rank, $suit, true);
            detection.score = 0.90;
            detection
        } ),*]
    };
}

fn armed_tracker(min_hits: usize) -> CardTracker {
    let mut tracker = CardTracker::new(Arc::new(game()), min_hits, 0.92, 1.6);
    let cards = bottom![("4", "S"), ("4", "D"), ("4", "C")];
    tracker.observe(&cards, 0.0);
    tracker.observe(&cards, 0.1);
    assert!(tracker.armed());
    tracker
}

#[test]
fn requires_three_bottom_cards_before_tracking() {
    let mut tracker = CardTracker::new(Arc::new(game()), 1, 0.92, 1.6);
    let hand = [det("A", "H", false), det("5", "H", false)];
    tracker.observe(&hand, 0.0);
    assert!(!tracker.armed());
    assert_eq!(tracker.snapshot().seen_count, 0);
    assert_eq!(tracker.snapshot().remaining, 54);

    let partial = bottom![("10", "S"), ("A", "H")];
    tracker.observe(&partial, 0.1);
    tracker.observe(&partial, 0.2);
    assert!(!tracker.armed());

    let full = bottom![("10", "S"), ("A", "H"), ("8", "H")];
    tracker.observe(&full, 0.3);
    assert!(!tracker.armed());
    tracker.observe(&full, 0.4);
    assert!(tracker.armed());
    assert_eq!(tracker.snapshot().bottom_ids.len(), 3);
}

#[test]
fn joker_bottom_is_tolerated_between_frames() {
    let mut tracker = CardTracker::new(Arc::new(game()), 1, 0.92, 1.6);
    let full = bottom![("BJ", ""), ("2", "H"), ("3", "H")];
    let partial = bottom![("2", "H"), ("3", "H")];
    tracker.observe(&full, 0.0);
    tracker.observe(&partial, 0.1);
    tracker.observe(&full, 0.2);
    assert!(tracker.armed());
    assert_eq!(tracker.snapshot().bottom_ids.len(), 3);
}

#[test]
fn bottom_cards_stay_in_remaining_until_played() {
    let mut tracker = CardTracker::new(Arc::new(game()), 1, 0.92, 1.6);
    let cards = bottom![("10", "S"), ("A", "H"), ("8", "H")];
    tracker.observe(&cards, 0.0);
    tracker.observe(&cards, 0.1);
    assert_eq!(tracker.snapshot().seen_count, 0);
    assert_eq!(tracker.snapshot().remaining, 54);
    assert_eq!(tracker.snapshot().counts["A"], 4);

    tracker.observe(&[det("A", "H", false)], 0.2);
    let snap = tracker.snapshot();
    assert_eq!(snap.seen_count, 1);
    assert_eq!(snap.remaining, 53);
    assert!(!snap.bottom_ids.contains("AH"));
}

#[test]
fn two_hits_and_instant_commit() {
    let mut tracker = armed_tracker(2);
    let mut low = det("2", "H", false);
    low.score = 0.80;
    tracker.observe(&[low.clone()], 1.0);
    assert_eq!(tracker.snapshot().seen_count, 0);
    tracker.observe(&[low], 1.1);
    assert_eq!(tracker.snapshot().seen_count, 1);

    let mut instant = armed_tracker(2);
    let mut high = det("K", "S", false);
    high.score = 0.99;
    instant.observe(&[high], 1.0);
    assert_eq!(instant.snapshot().seen_count, 1);
}

#[test]
fn reset_clears_round_and_waits_for_bottom_again() {
    let mut tracker = armed_tracker(1);
    tracker.observe(&[det("3", "H", false)], 1.0);
    tracker.reset();
    assert!(!tracker.armed());
    assert_eq!(tracker.snapshot().seen_count, 0);
    assert_eq!(tracker.snapshot().counts["3"], 4);
}

#[test]
fn game_without_bottom_starts_armed() {
    let mut game = game();
    game.bottom = None;
    let tracker = CardTracker::new(Arc::new(game), 1, 0.92, 1.6);
    assert!(tracker.armed());
}
