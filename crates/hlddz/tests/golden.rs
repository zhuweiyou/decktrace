use std::collections::BTreeSet;
use std::path::PathBuf;

use hlddz::{HlddzRecognizer, profile};
use tracking::RecognitionResult;

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/hlddz")
}

fn recognize(frame: &str) -> Option<RecognitionResult> {
    let path = samples_dir().join(frame);
    if !path.exists() {
        return None;
    }
    let game = profile();
    let recognizer = HlddzRecognizer::new(&game).expect("recognizer");
    let image = image::open(&path).expect("image").to_rgb8();
    Some(recognizer.recognize(&image, true))
}

/// 手牌区（窗口纵向 70% 以下）的牌面集合。
fn hand_ids(result: &RecognitionResult) -> BTreeSet<String> {
    result
        .detections
        .iter()
        .filter(|det| det.bbox.1 as f32 > result.frame_height as f32 * 0.70)
        .map(|det| det.card_id())
        .collect()
}

#[test]
fn hand_of_mid_round_matches_expectation() {
    // 15 张手牌 + 左家 10♣10♦ + 中间 A♠A♥ + 顶部底牌 3♥
    let Some(result) = recognize("frame_000032.png") else {
        return;
    };
    let expected: BTreeSet<String> = [
        "2H", "QS", "QC", "QD", "JS", "JH", "JC", "JD", "10S", "10H", "9S", "9H", "9C", "9D",
        "3C",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(hand_ids(&result), expected);

    let all: BTreeSet<String> = result.detections.iter().map(|det| det.card_id()).collect();
    assert!(all.contains("10C") && all.contains("10D"), "左家出牌");
    assert!(all.contains("AS") && all.contains("AH"), "中间出牌");
}

#[test]
fn hand_of_first_round_matches_expectation() {
    // 17 张手牌：2♠2♥2♣2♦A♣8♣8♦7♠7♥7♣7♦6♠6♥5♠5♥5♣4♠
    let Some(result) = recognize("frame_000093.png") else {
        return;
    };
    let expected: BTreeSet<String> = [
        "2S", "2H", "2C", "2D", "AC", "8C", "8D", "7S", "7H", "7C", "7D", "6S", "6H", "5S", "5H",
        "5C", "4S",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(hand_ids(&result), expected);
}

#[test]
fn hand_of_late_round_matches_expectation() {
    // 13 张手牌：K♠K♣K♦J♥J♣10♥10♣10♦8♠8♥8♣8♦3♥
    let Some(result) = recognize("frame_000146.png") else {
        return;
    };
    let expected: BTreeSet<String> = [
        "KS", "KC", "KD", "JH", "JC", "10H", "10C", "10D", "8S", "8H", "8C", "8D", "3H",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(hand_ids(&result), expected);
}
