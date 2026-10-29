use std::collections::BTreeSet;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let game = wzddz::profile();
    let rec = wzddz::WzddzRecognizer::new(&game)?;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut files = 0usize; let mut total = 0usize;
    for path in std::env::args().skip(1) {
        let image = match image::open(&path) { Ok(i)=>i.to_rgb8(), Err(_)=>continue };
        files += 1;
        let r = rec.recognize(&image, true);
        total += r.detections.len();
        for d in &r.detections { if d.score>=0.60 { seen.insert(d.card_id()); } }
    }
    let ranks=["2","A","K","Q","J","10","9","8","7","6","5","4","3"]; let suits=["H","D","S","C"];
    let mut all=vec![];
    for r in ranks { for s in suits { all.push(format!("{r}{s}")); } }
    all.push("BJ".into()); all.push("SJ".into());
    let missing: Vec<&String>=all.iter().filter(|c| !seen.contains(*c)).collect();
    println!("frames={files} 平均检出/帧={:.1} 覆盖={}/{}", total as f64/files.max(1) as f64, seen.len(), all.len());
    println!("缺失({}): {}", missing.len(), missing.iter().map(|s|s.as_str()).collect::<Vec<_>>().join(" "));
    Ok(())
}
