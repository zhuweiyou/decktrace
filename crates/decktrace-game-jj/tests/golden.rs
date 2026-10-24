use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use decktrace_core::RecognitionResult;
use decktrace_game_jj::{JjRecognizer, profile};

fn shots_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../shots")
}

fn recognize(name: &str) -> Option<RecognitionResult> {
    let path = shots_dir().join(name);
    if !path.exists() {
        return None;
    }
    let game = profile();
    let recognizer = JjRecognizer::new(&game).expect("recognizer");
    let image = image::open(&path).expect("image").to_rgb8();
    Some(recognizer.recognize(&image, true))
}

fn id_counts(result: &RecognitionResult) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for det in &result.detections {
        *counts.entry(det.card_id()).or_insert(0) += 1;
    }
    counts
}

fn bottom_ids(result: &RecognitionResult) -> BTreeSet<String> {
    result
        .detections
        .iter()
        .filter(|det| det.is_bottom)
        .map(|det| det.card_id())
        .collect()
}

#[test]
fn round_now_matches_python_oracle() {
    let Some(result) = recognize("round_now.png") else {
        return;
    };
    let expected: BTreeSet<&str> = [
        "10C", "10D", "10S", "2C", "2H", "3C", "3H", "4C", "4H", "5C", "5D", "5H", "5S", "6C",
        "6H", "6S", "7D", "8H", "9D", "9H", "AD", "AH", "AS", "BJ", "JS", "KS",
    ]
    .into_iter()
    .collect();
    let actual: BTreeSet<String> = id_counts(&result).into_keys().collect();
    assert_eq!(actual, expected.into_iter().map(str::to_string).collect());
    assert_eq!(result.detections.len(), 26);
    assert_eq!(
        bottom_ids(&result),
        ["10S", "AH", "8H"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
}

#[test]
fn round_live_face_down_bottom() {
    let Some(result) = recognize("round_live.png") else {
        return;
    };
    assert_eq!(result.detections.len(), 17);
    assert!(bottom_ids(&result).is_empty());
}

#[test]
fn round_live2_matches_python_oracle() {
    let Some(result) = recognize("round_live2.png") else {
        return;
    };
    assert_eq!(result.detections.len(), 21);
    assert_eq!(
        bottom_ids(&result),
        ["7C", "9D", "9H"].into_iter().map(str::to_string).collect()
    );
}

#[test]
fn bj_bottom_and_hand_jokers() {
    let Some(result) = recognize("bj_bottom_real.png") else {
        return;
    };
    let counts = id_counts(&result);
    assert_eq!(result.detections.len(), 20);
    assert_eq!(counts.get("BJ"), Some(&2));
    assert_eq!(counts.get("SJ"), Some(&1));
    assert_eq!(
        bottom_ids(&result),
        ["BJ", "JC", "JH"].into_iter().map(str::to_string).collect()
    );
}

#[test]
fn live_now2_matches_python_oracle() {
    let Some(result) = recognize("live_now2.png") else {
        return;
    };
    let expected: BTreeSet<&str> = [
        "10C", "2D", "3C", "3H", "4C", "5H", "6D", "7H", "8D", "8H", "9C", "AH", "AS", "BJ", "JH",
        "JS", "KC", "KD", "QC", "QD",
    ]
    .into_iter()
    .collect();
    let actual: BTreeSet<String> = id_counts(&result).into_keys().collect();
    assert_eq!(actual, expected.into_iter().map(str::to_string).collect());
    assert_eq!(result.detections.len(), 23);
    assert_eq!(
        bottom_ids(&result),
        ["10C", "8D", "AH"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
}

#[test]
fn joker_miss_matches_python_oracle() {
    let Some(result) = recognize("joker_miss.png") else {
        return;
    };
    let expected: BTreeSet<&str> = [
        "10D", "10S", "2S", "3C", "3D", "4S", "5C", "5H", "5S", "6D", "6S", "7D", "7H", "8C", "8H",
        "9C", "AD", "JC", "JD", "JS", "QH", "SJ",
    ]
    .into_iter()
    .collect();
    let actual: BTreeSet<String> = id_counts(&result).into_keys().collect();
    assert_eq!(actual, expected.into_iter().map(str::to_string).collect());
    assert_eq!(
        bottom_ids(&result),
        ["AD", "SJ"].into_iter().map(str::to_string).collect()
    );
}
