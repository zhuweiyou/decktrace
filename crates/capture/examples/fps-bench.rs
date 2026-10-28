//! Windows 采集性能基准：直接循环调用原始抓帧，测后端吞吐（不受帧率节流影响）。
//!
//! ```text
//! cargo run --release -p capture --example fps-bench -- [窗口标题子串] [帧数] [--paced 15]
//! cargo run --release -p capture --example fps-bench -- "zhuweiyou/decktrace" 200
//! cargo run --release -p capture --example fps-bench -- "微信" 150 --paced 15
//! ```
//!
//! 不给标题时自动挑选面积最大的可见窗口。默认测“连续抓帧吞吐”（不受帧率限制）；
//! 加 `--paced <fps>` 会再用带节流的 `read()` 跑一遍，验证能稳定跑到目标帧率。
//! 输出每帧延迟的均值/中位/P95/最小/最大与折算 FPS，并顺带校验画面不是纯色。

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};

    use capture::windows::WindowsCaptureStream;
    use capture::{CaptureStream, Platform, PlatformImpl, WindowInfo};
    use tracking::WindowSelector;

    let mut argv = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut paced: Option<f32> = None;
    while let Some(arg) = argv.next() {
        if arg == "--paced" {
            paced = Some(argv.next().ok_or("--paced 需要帧率")?.parse()?);
        } else {
            positional.push(arg);
        }
    }
    let title = positional.first().cloned();
    let frames: usize = positional
        .get(1)
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(120)
        .max(10);

    let platform = PlatformImpl::new()?;
    let windows = platform.window_service().list_windows(false)?;

    let window: WindowInfo = if let Some(title) = title {
        platform
            .window_service()
            .find_window(&WindowSelector::new(title))?
    } else {
        windows
            .iter()
            .filter(|w| w.width >= 320.0 && w.height >= 240.0)
            .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
            .cloned()
            .ok_or("没有可用窗口")?
    };

    println!(
        "目标窗口 #{} {} / {:?} {:.0}x{:.0}",
        window.id, window.app, window.title, window.width, window.height
    );

    // 用采集流本身（资源跨帧复用）测量，等同于应用实际路径。
    let mut stream = WindowsCaptureStream::new(window.id, 60.0)?;

    // 预热，避免首次 GDI 设备/字体缓存初始化计入样本。
    for _ in 0..5 {
        let _ = stream.grab_now()?;
    }

    let mut samples = Vec::with_capacity(frames);
    let mut last_image = None;
    for _ in 0..frames {
        let t0 = Instant::now();
        let frame = stream.grab_now()?;
        samples.push(t0.elapsed());
        last_image = Some(frame.image);
    }

    let image = last_image.expect("至少一帧");
    let (w, h) = (image.width(), image.height());

    // 纯色检测：采样若干像素判断颜色方差，全黑/全白通常意味着抓到了空图。
    let mut min = 255u8;
    let mut max = 0u8;
    let step = (w as usize * h as usize / 4096).max(1);
    for (i, px) in image.pixels().enumerate() {
        if i % step == 0 {
            let gray = ((u32::from(px[0]) * 299
                + u32::from(px[1]) * 587
                + u32::from(px[2]) * 114)
                / 1000) as u8;
            min = min.min(gray);
            max = max.max(gray);
        }
    }

    let total: Duration = samples.iter().sum();
    let mut sorted = samples.clone();
    sorted.sort();
    let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let mean = total / samples.len() as u32;
    let fps = 1.0 / mean.as_secs_f64();

    println!("帧数={} 尺寸={}x{}", samples.len(), w, h);
    println!(
        "单帧延迟: 均值={:.2}ms 中位={:.2}ms P95={:.2}ms 最小={:.2}ms 最大={:.2}ms",
        mean.as_secs_f64() * 1000.0,
        pct(0.5).as_secs_f64() * 1000.0,
        pct(0.95).as_secs_f64() * 1000.0,
        sorted[0].as_secs_f64() * 1000.0,
        sorted[sorted.len() - 1].as_secs_f64() * 1000.0,
    );
    println!(
        "连续抓帧吞吐: {:.1} FPS（总耗时 {:.3}s）",
        fps,
        total.as_secs_f64()
    );
    println!("画面灰度范围: {}..{}（跨度 {}）", min, max, max - min);
    if max - min < 8 {
        println!("警告：画面接近纯色，可能抓到空白/黑屏");
    }

    // 可选：验证带帧率节流的 read() 实际能稳定跑到目标帧率（应用默认 15fps）。
    if let Some(target) = paced {
        let mut paced_stream = WindowsCaptureStream::new(window.id, target)?;
        paced_stream.start()?;
        let t0 = Instant::now();
        let mut got = 0u32;
        let mut idle = 0u32;
        while got < frames as u32 && idle < frames as u32 * 20 {
            match paced_stream.read(Duration::from_millis(200))? {
                Some(_) => {
                    got += 1;
                    idle = 0;
                }
                None => idle += 1,
            }
        }
        let elapsed = t0.elapsed();
        println!(
            "节流读取: 目标 {}fps，实际 {:.1} FPS，{} 帧 / {:.2}s",
            target,
            got as f64 / elapsed.as_secs_f64(),
            got,
            elapsed.as_secs_f64()
        );
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("fps-bench only runs on Windows");
}
