//! 真实窗口截屏集成测试（仅 Windows）。
//!
//! 从当前桌面上任意挑一个可见窗口截一帧，校验：
//! - 能成功拿到帧
//! - 帧尺寸与图像尺寸一致且非零
//! - 画面不是纯色（区分“真的截到内容”与“黑屏/空白”）
//!
//! 会依次尝试多个候选窗口，只要有一个截成功即通过；全部失败才报错并列出原因。
//! 这样不依赖具体某个应用（浏览器/微信/记事本皆可），符合“随便截个窗口”。

#![cfg(target_os = "windows")]

use std::time::Duration;

use capture::{CaptureError, Platform, PlatformImpl, RawFrame, WindowInfo};

fn try_capture(platform: &dyn Platform, window: &WindowInfo) -> capture::Result<RawFrame> {
    let mut stream = platform.create_capture_stream(window, 5.0)?;
    stream.start()?;
    let frame = stream.read(Duration::from_secs(3))?.ok_or_else(|| {
        CaptureError::Capture("3 秒内没有拿到帧（read 返回 None）".to_string())
    })?;
    stream.stop();
    Ok(frame)
}

/// 采样灰度极差，用于判断是否几乎纯色。
fn gray_span(image: &image::RgbImage) -> u8 {
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w == 0 || h == 0 {
        return 0;
    }
    let step = (w * h / 4096).max(1);
    let mut min = 255u8;
    let mut max = 0u8;
    for (i, px) in image.pixels().enumerate() {
        if i % step == 0 {
            let gray = ((u32::from(px[0]) * 299 + u32::from(px[1]) * 587 + u32::from(px[2]) * 114)
                / 1000) as u8;
            min = min.min(gray);
            max = max.max(gray);
        }
    }
    max - min
}

#[test]
fn captures_some_real_window() {
    let platform = PlatformImpl::new().expect("创建 Windows 平台失败");
    let windows = platform
        .window_service()
        .list_windows(false)
        .expect("枚举窗口失败");
    assert!(
        !windows.is_empty(),
        "当前桌面没有可见窗口，无法进行截屏测试（请在带桌面的 Windows 会话中运行）"
    );

    let mut candidates: Vec<WindowInfo> = windows
        .into_iter()
        .filter(|w| w.width >= 200.0 && w.height >= 150.0)
        .collect();
    candidates.sort_by(|a, b| (b.width * b.height).total_cmp(&(a.width * a.height)));

    let mut errors = Vec::new();
    for window in &candidates {
        match try_capture(&platform, window) {
            Ok(frame) => {
                assert!(frame.width > 0 && frame.height > 0, "帧尺寸不应为 0");
                assert_eq!(
                    (frame.width, frame.height),
                    (frame.image.width(), frame.image.height()),
                    "帧尺寸应与图像尺寸一致"
                );
                let span = gray_span(&frame.image);
                if span < 8 {
                    // 透明/纯色覆盖层（如 TextInputHost、候选项）截出来就是黑的，跳过换下一个。
                    errors.push(format!(
                        "  #{:x} {:?}：画面纯色（灰度跨度 {span}），跳过",
                        window.id, window.title
                    ));
                    continue;
                }
                println!(
                    "截屏成功：{} → {}x{}，灰度跨度={span}",
                    window, frame.width, frame.height
                );
                return;
            }
            Err(err) => errors.push(format!("  #{:x} {:?}：{err}", window.id, window.title)),
        }
    }

    panic!(
        "尝试了 {} 个窗口，全部截屏失败：\n{}",
        candidates.len(),
        errors.join("\n")
    );
}
