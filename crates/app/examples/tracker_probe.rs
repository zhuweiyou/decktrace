//! 无头调牌工具：把 adb 实时帧或离线截图送进「识别 + 记牌状态机」，逐帧打印快照。
//!
//! 不启动 GUI，方便在终端里验证识别与底牌门控是否正确。
//!
//! ```text
//! # 离线回放（按给定顺序喂帧，min_hits=1，适合静态单帧）
//! cargo run --release -p app --example tracker_probe -- \
//!     --replay a.png b.png b.png
//!
//! # adb 实时（默认欢乐斗地主，2 帧确认）
//! cargo run --release -p app --example tracker_probe -- \
//!     --adb --serial 192.168.0.108:5555 --fps 2 --frames 40
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use capture::Platform;
use tracking::{CardTracker, Detection, SharedRecognizer};

fn main() -> Result<()> {
    let mut adb = false;
    let mut serial: Option<String> = None;
    let mut crop: Option<String> = None;
    let mut fps = 2.0f32;
    let mut frames = 40usize;
    let mut game_id = "hlddz_mobile".to_string();
    let mut min_hits = 1usize;
    let mut replay: Vec<String> = Vec::new();
    let mut dump: Option<String> = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--adb" => adb = true,
            "--serial" => serial = it.next(),
            "--crop" => crop = it.next(),
            "--fps" => fps = parse(it.next(), "--fps")?,
            "--frames" => frames = parse(it.next(), "--frames")?,
            "--game" => game_id = it.next().ok_or_else(|| anyhow!("--game 缺值"))?,
            "--min-hits" => min_hits = parse(it.next(), "--min-hits")?,
            "--dump" => dump = it.next(),
            "--replay" => {
                replay.extend(it.by_ref());
            }
            other => return Err(anyhow!("未知参数：{other}")),
        }
    }

    let game = app::get_game(&game_id).map_err(|err| anyhow!(err))?;
    let recognizer = app::create_recognizer(&game).map_err(|err| anyhow!(err))?;
    // adb 单帧截图约 2s，远大于 2 帧确认的有效期，所以默认 1 帧即入账；
    // 需要更严格可用 --min-hits 2（仅在高帧率采集下有意义）。
    let tracker_hits = min_hits;
    let mut tracker = CardTracker::new(Arc::new(game), tracker_hits, 0.92, 6.0);

    if !replay.is_empty() {
        println!("== 回放 {} 帧 ==", replay.len());
        let mut t = 0.0f64;
        for path in &replay {
            let image = image::open(path)?.to_rgb8();
            report(&recognizer, &mut tracker, &image, t, path);
            t += 0.5;
        }
        return Ok(());
    }

    if !adb {
        return Err(anyhow!("请指定 --adb 或 --replay <图片...>"));
    }

    let platform = capture::adb::AdbPlatform::new(serial, crop)?;
    let windows = platform.window_service().list_windows(false)?;
    let window = windows.into_iter().next().ok_or_else(|| anyhow!("未发现设备屏幕"))?;
    println!(
        "[adb] 设备 {}x{} @ ({:.0},{:.0})",
        window.width, window.height, window.x, window.y
    );
    let mut stream = platform.create_capture_stream(&window, fps)?;
    stream.start()?;
    let started = Instant::now();
    let mut got = 0usize;
    while got < frames {
        match stream.read(Duration::from_millis(1500)) {
            Ok(Some(frame)) => {
                got += 1;
                let now = started.elapsed().as_secs_f64();
                println!("--- 第 {got} 帧（{now:.1}s）---");
                if let Some(dir) = dump.as_deref() {
                    std::fs::create_dir_all(dir).ok();
                    let path = std::path::Path::new(dir).join(format!("live_{got:03}.png"));
                    if let Err(err) = frame.image.save(&path) {
                        eprintln!("落帧失败：{err}");
                    } else {
                        println!("  落帧 -> {}", path.display());
                    }
                }
                report(&recognizer, &mut tracker, &frame.image, now, "");
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("读取失败：{err}");
                break;
            }
        }
    }
    stream.stop();
    Ok(())
}

fn report(
    recognizer: &SharedRecognizer,
    tracker: &mut CardTracker,
    image: &image::RgbImage,
    now: f64,
    tag: &str,
) {
    let result = recognizer.recognize(image, true);
    let committed: Vec<Detection> = tracker.observe(&result.detections, now);
    let snapshot = tracker.snapshot();
    let new_ids: Vec<String> = committed.iter().map(Detection::card_id).collect();
    let remaining: Vec<String> = snapshot
        .counts
        .iter()
        .filter(|(_, left)| **left > 0)
        .map(|(rank, left)| format!("{rank}×{left}"))
        .collect();
    println!(
        "  识别 {} 张 | armed={} seen={} 剩余={} | 底牌={:?} | 本帧新增={:?}\n  剩余点数: {}",
        result.detections.len(),
        snapshot.armed,
        snapshot.seen_count,
        snapshot.remaining,
        snapshot.bottom_ids,
        new_ids,
        remaining.join(" ")
    );
    if !tag.is_empty() {
        println!("  <- {tag}");
    }
}

fn parse<T: std::str::FromStr>(value: Option<String>, flag: &str) -> Result<T> {
    value
        .ok_or_else(|| anyhow!("{flag} 缺值"))?
        .parse()
        .map_err(|_| anyhow!("{flag} 需要合法数字"))
}
