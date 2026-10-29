use wzddz::{WzddzRecognizer, profile};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for p in std::env::args().skip(1) {
        let game = profile();
        let rec = WzddzRecognizer::new(&game)?;
        let i = image::open(&p)?.to_rgb8();
        let ranks = rec.detect_ranks(&i);
        let mut v: Vec<String> = ranks.iter().map(|d| d.rank.clone()).collect();
        v.sort();
        println!("{p} n={} {}", v.len(), v.join(","));
    }
    Ok(())
}
