//! 持续截取任意窗口，按画面差异去重落盘，供识别开发取样。
//!
//! ```text
//! cargo run --release -p capture --example watch -- --title 欢乐斗地主 --fps 2
//! cargo run --release -p capture --example watch -- --title 欢乐斗地主 --duration 30
//! ```
//!
//! 输出目录（默认 `samples/hlddz`）：
//! - `frame_000001.png` … 画面发生明显变化时保存，序号连续、不覆盖历史帧
//! - `latest.png`         最近一次保存的画面，方便直接看
//! - `index.tsv`          序号、时间戳、尺寸、差分值，便于对齐牌局事件
//!
//! Ctrl-C 退出；已经落盘的帧不受影响。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use capture::{CaptureStream, Platform, PlatformImpl, WindowInfo};
use tracking::WindowSelector;

// 缩略网格：够细能区分“局部计时器跳动”和“发牌/出牌”，又不至于被噪点触发。
const THUMB_W: u32 = 32;
const THUMB_H: u32 = 20;
// 单个网格块灰度差超过该值才算“变了”。
const BLOCK_DELTA: u8 = 10;

struct Args {
    title: String,
    fps: f32,
    out: PathBuf,
    diff: f32,
    min_blocks: usize,
    snap: f32,
    max_frames: u64,
    duration: Option<Duration>,
    once: bool,
}

fn usage() -> &'static str {
    "用法：watch --title <窗口标题> [--fps 2] [--out samples/hlddz] [--diff 2.0]
            [--min-blocks 64] [--snap 2] [--max-frames N] [--duration 秒] [--once]

  --title     窗口标题子串，默认“欢乐斗地主”
  --fps       采集帧率，默认 2
  --out       输出目录，默认 samples/hlddz（相对当前目录）
  --diff      平均灰度差兜底阈值（0-255），默认 10.0；整屏切换时触发
  --min-blocks 明显变化的网格块数，默认 64（共 640 块）；调小更敏感
  --snap      定时兜底间隔秒数，默认 2；0 表示完全按画面变化保存
  --max-frames 保存到 N 帧后自动退出，0 表示不限
  --duration  采集时长，省略则一直运行
  --once      只截一张就退出，用于确认窗口和权限"
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        title: "欢乐斗地主".to_string(),
        fps: 2.0,
        out: PathBuf::from("samples/hlddz"),
        diff: 10.0,
        min_blocks: 64,
        snap: 2.0,
        max_frames: 0,
        duration: None,
        once: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--title" => args.title = it.next().ok_or("--title 缺少值")?,
            "--fps" => {
                args.fps = it
                    .next()
                    .ok_or("--fps 缺少值")?
                    .parse()
                    .map_err(|_| "--fps 需要数字")?
            }
            "--out" => args.out = PathBuf::from(it.next().ok_or("--out 缺少值")?),
            "--diff" => {
                args.diff = it
                    .next()
                    .ok_or("--diff 缺少值")?
                    .parse()
                    .map_err(|_| "--diff 需要数字")?
            }
            "--min-blocks" => {
                args.min_blocks = it
                    .next()
                    .ok_or("--min-blocks 缺少值")?
                    .parse()
                    .map_err(|_| "--min-blocks 需要整数")?
            }
            "--snap" => {
                args.snap = it
                    .next()
                    .ok_or("--snap 缺少值")?
                    .parse()
                    .map_err(|_| "--snap 需要秒数")?
            }
            "--max-frames" => {
                args.max_frames = it
                    .next()
                    .ok_or("--max-frames 缺少值")?
                    .parse()
                    .map_err(|_| "--max-frames 需要整数")?
            }
            "--duration" => {
                let secs: f64 = it
                    .next()
                    .ok_or("--duration 缺少值")?
                    .parse()
                    .map_err(|_| "--duration 需要秒数")?;
                args.duration = Some(Duration::from_secs_f64(secs.max(0.1)));
            }
            "--once" => args.once = true,
            "-h" | "--help" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("未知参数：{other}\n\n{}", usage())),
        }
    }
    Ok(args)
}

/// 16×10 灰度块均值，够用且比逐像素比对便宜。
fn thumbnail(image: &image::RgbImage) -> Vec<u8> {
    let (width, height) = (image.width(), image.height());
    let mut out = vec![0u8; (THUMB_W * THUMB_H) as usize];
    for ty in 0..THUMB_H {
        let y0 = ty * height / THUMB_H;
        let y1 = ((ty + 1) * height / THUMB_H).max(y0 + 1);
        for tx in 0..THUMB_W {
            let x0 = tx * width / THUMB_W;
            let x1 = ((tx + 1) * width / THUMB_W).max(x0 + 1);
            let mut sum = 0u64;
            let mut count = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let pixel = image.get_pixel(x, y);
                    sum += (u64::from(pixel[0]) * 299
                        + u64::from(pixel[1]) * 587
                        + u64::from(pixel[2]) * 114)
                        / 1000;
                    count += 1;
                }
            }
            out[(ty * THUMB_W + tx) as usize] = (sum / count.max(1)) as u8;
        }
    }
    out
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f32 {
    let sum: u32 = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u32).sum();
    sum as f32 / a.len().max(1) as f32
}

/// 明显变化的网格块数：计时器、粒子这类局部动画只会影响一两块。
fn changed_blocks(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .filter(|(x, y)| x.abs_diff(**y) >= BLOCK_DELTA)
        .count()
}

/// 接续已有帧序号，避免重跑时覆盖历史样本。
fn next_frame_index(dir: &Path) -> u64 {
    let mut max = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(rest) = name.strip_prefix("frame_")
                && let Some(num) = rest.strip_suffix(".png")
                && let Ok(value) = num.parse::<u64>()
            {
                max = max.max(value);
            }
        }
    }
    max + 1
}

fn open_index(dir: &Path) -> std::io::Result<File> {
    let path = dir.join("index.tsv");
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    if file.metadata()?.len() == 0 {
        writeln!(file, "seq\tts_ms\twidth\theight\tdiff")?;
    }
    Ok(file)
}

fn find_window(platform: &dyn Platform, title: &str) -> capture::Result<WindowInfo> {
    platform
        .window_service()
        .find_window(&WindowSelector::new(title))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    std::fs::create_dir_all(&args.out)?;
    let platform = PlatformImpl::new()?;

    let mut stream: Option<Box<dyn CaptureStream>> = None;
    let mut next_seq = next_frame_index(&args.out);
    let mut index = open_index(&args.out)?;
    let mut prev_thumb: Option<Vec<u8>> = None;
    let mut saved = 0u64;
    let mut skipped = 0u64;
    let mut last_saved = Instant::now() - Duration::from_secs(3600);
    let started = Instant::now();

    while args.duration.is_none_or(|limit| started.elapsed() < limit) {
        if stream.is_none() {
            match find_window(&platform, &args.title) {
                Ok(window) => {
                    println!("[watch] 锁定窗口 {window}");
                    match platform.create_capture_stream(&window, args.fps) {
                        Ok(mut created) => match created.start() {
                            Ok(()) => stream = Some(created),
                            Err(err) => eprintln!("[watch] 启动采集失败：{err}"),
                        },
                        Err(err) => eprintln!("[watch] 创建采集流失败：{err}"),
                    }
                }
                Err(err) => {
                    eprintln!("[watch] 未找到窗口：{err}");
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
        }

        let Some(active) = stream.as_mut() else {
            continue;
        };
        let wait = if args.once {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(1500)
        };
        let frame = match active.read(wait) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(err) => {
                eprintln!("[watch] 采集中断（{err}），重新查找窗口");
                stream = None;
                continue;
            }
        };

        let thumb = thumbnail(&frame.image);
        let (blocks, mean) = prev_thumb.as_ref().map_or((0, 0.0), |prev| {
            (changed_blocks(prev, &thumb), mean_abs_diff(prev, &thumb))
        });
        // 触发条件：首帧、明显变化（发牌/出牌/结算）、或到点兜底。
        let big_change = blocks >= args.min_blocks || mean >= args.diff;
        let due = args.snap > 0.0 && last_saved.elapsed().as_secs_f32() >= args.snap;
        let changed = args.once || prev_thumb.is_none() || big_change || due;
        if changed {
            let path = args.out.join(format!("frame_{next_seq:06}.png"));
            frame.image.save(&path)?;
            std::fs::copy(&path, args.out.join("latest.png"))?;
            let ts_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let diff_text = if prev_thumb.is_none() {
                "first".to_string()
            } else {
                format!("{mean:.1}/b{blocks}")
            };
            writeln!(
                index,
                "{next_seq}\t{ts_ms}\t{}\t{}\t{diff_text}",
                frame.width, frame.height
            )?;
            println!(
                "[save] {} {}x{} diff={diff_text}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                frame.width,
                frame.height
            );
            next_seq += 1;
            saved += 1;
            last_saved = Instant::now();
            prev_thumb = Some(thumb);
            if args.once || (args.max_frames > 0 && saved >= args.max_frames) {
                break;
            }
        } else {
            skipped += 1;
            if skipped % 20 == 0 {
                println!("[idle] 画面未变化，已跳过 {skipped} 帧（共保存 {saved}）");
            }
        }
    }

    if let Some(mut active) = stream {
        active.stop();
    }
    println!(
        "[done] 保存 {saved} 帧，跳过 {skipped} 帧 → {}",
        args.out.display()
    );
    Ok(())
}
