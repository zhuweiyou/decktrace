use jjddz::{JjddzRecognizer, profile};

fn main() {
    let path = std::env::args().nth(1).expect("usage: recognize <image>");
    let game = profile();
    let recognizer = JjddzRecognizer::new(&game).expect("recognizer");
    let image = image::open(&path).expect("image").to_rgb8();
    let result = recognizer.recognize(&image, true);
    println!(
        "{} cards {:.0}ms",
        result.detections.len(),
        result.elapsed_ms
    );
    let mut ids: Vec<_> = result
        .detections
        .iter()
        .map(|d| (d.card_id(), d.is_bottom, d.score))
        .collect();
    ids.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, bottom, score) in ids {
        println!("{id:4} bottom={bottom} score={score:.2}");
    }
}
