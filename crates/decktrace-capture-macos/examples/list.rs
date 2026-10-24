use decktrace_capture::Platform;
use decktrace_capture_macos::MacPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let platform = MacPlatform::new()?;
    let windows = platform.window_service().list_windows(true)?;
    for window in windows {
        if window.layer == 0 && window.width >= 80.0 && window.height >= 80.0 {
            println!(
                "#{:<6} {:<20} {:?} {} {}",
                window.id,
                window.app,
                window.title,
                window.size(),
                window.position()
            );
        }
    }
    Ok(())
}
