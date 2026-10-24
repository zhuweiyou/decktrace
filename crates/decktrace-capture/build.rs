use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // screencapturekit/apple-cf 的 Swift 桥接需要 Swift 运行时兼容库。
    // Command Line Tools 下 xcrun 找到的 swiftc 路径与链接器默认搜索路径不一定一致，
    // 这里显式把工具链的 Swift 库目录加进 link search。
    if let Ok(output) = Command::new("xcrun").args(["--find", "swiftc"]).output()
        && output.status.success()
    {
        let swiftc = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        if let Some(toolchain) = swiftc.parent().and_then(|bin| bin.parent()) {
            let lib = toolchain.join("lib/swift/macosx");
            if lib.exists() {
                println!("cargo:rustc-link-search=native={}", lib.display());
            }
            // Swift 运行时在 dyld 缓存里，rpath 指向系统路径，避免加载 CLT 的旧副本。
            println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
