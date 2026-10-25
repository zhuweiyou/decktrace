//! Windows 冒烟测试：列出窗口，可选按标题抓一帧。
//!
//! ```text
//! cargo run --target x86_64-pc-windows-gnu -p capture --example windows-smoke -- "Notepad"
//! ```

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use capture::{Platform, PlatformImpl};
    use tracking::WindowSelector;

    let platform = PlatformImpl::new()?;
    let windows = platform.window_service().list_windows(false)?;
    println!("windows={}", windows.len());
    for window in &windows {
        println!(
            "#{:016x} pid={} app={:?} title={:?} {}x{}",
            window.id, window.pid, window.app, window.title, window.width, window.height
        );
    }
    let Some(title) = std::env::args().nth(1) else {
        return Ok(());
    };
    let selector = WindowSelector::new(title);
    let window = platform.window_service().find_window(&selector)?;
    let mut stream = platform.create_capture_stream(&window, 5.0)?;
    stream.start()?;
    let frame = stream
        .read(Duration::from_secs(2))?
        .ok_or("no frame captured")?;
    println!(
        "captured {}x{} seq={}",
        frame.width, frame.height, frame.seq
    );
    frame.image.save("windows-smoke.png")?;
    stream.stop();
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("windows-smoke only runs on Windows");
}
