fn main() {
    for p in std::env::args().skip(1) {
        match image::open(&p) {
            Ok(i) => { let (e,c)=wzddz::settlement_scores(&i.to_rgb8()); println!("{p} exit={:.3} cont={:.3} settled={}", e,c,wzddz::detect_settlement(&i.to_rgb8())); }
            Err(err) => println!("{p} err {err}"),
        }
    }
}
