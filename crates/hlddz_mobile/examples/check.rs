//! 手机版调试：对单张截图跑识别，打印检测结果并输出标注图。
//!
//! ```text
//! cargo run --release -p hlddz_mobile --example check -- samples/frame_000032.png
//! ```

use hlddz_mobile::{HlddzRecognizer, profile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("用法：check <截图路径> [--no-joker]")?;
    let jokers = !std::env::args().any(|a| a == "--no-joker");
    let game = profile();
    let recognizer = HlddzRecognizer::new(&game)?;
    let image = image::open(&path)?.to_rgb8();
    let result = recognizer.recognize(&image, jokers);
    println!(
        "detections={} elapsed={:.1}ms {}x{}",
        result.detections.len(),
        result.elapsed_ms,
        result.frame_width,
        result.frame_height
    );
    for det in &result.detections {
        println!(
            "{:>4} {:?} score={:.2} rank={:.2} suit={:.2} face={:.2} bottom={}",
            det.display,
            det.bbox,
            det.score,
            det.rank_score,
            det.suit_score,
            det.face_score,
            det.is_bottom
        );
    }
    let annotated = hlddz_mobile::recognition::annotate(&image, &result.detections);
    let out = std::path::Path::new(&path).with_extension("mobile.annot.png");
    annotated.save(&out)?;
    println!("annotated -> {}", out.display());
    Ok(())
}
