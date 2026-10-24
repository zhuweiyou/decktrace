use decktrace_game_jj::profile;
use decktrace_recognition::CardRecognizer;

fn main() {
    let path = std::env::args().nth(1).expect("usage: recognize <image>");
    let game = profile();
    let recognizer = CardRecognizer::new(&game).expect("recognizer");
    let image = image::open(&path).expect("image").to_rgb8();
    let result = recognizer.recognize(&image, true);
    println!(
        "{}x{} {} cards {:.0}ms",
        result.frame_width,
        result.frame_height,
        result.detections.len(),
        result.elapsed_ms
    );
    for det in &result.detections {
        println!(
            "{:4} box={:?} score={:.2} bottom={}",
            det.card_id(),
            det.bbox,
            det.score,
            det.is_bottom
        );
    }
}
