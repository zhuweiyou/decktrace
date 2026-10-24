use std::time::Duration;

use decktrace_capture::Platform;
use decktrace_capture_macos::MacPlatform;
use decktrace_core::WindowSelector;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let platform = MacPlatform::new()?;
    let window = platform
        .window_service()
        .find_window(&WindowSelector::new("JJ斗地主"))?;
    println!("window: {window}");
    let mut stream = platform.create_capture_stream(&window, 15.0)?;
    stream.start()?;
    let frame = stream.read(Duration::from_secs(5))?.ok_or("no frame")?;
    stream.stop();
    println!("frame {}x{} seq={}", frame.width, frame.height, frame.seq);
    frame.image.save("/tmp/rust_shot.png")?;
    Ok(())
}
