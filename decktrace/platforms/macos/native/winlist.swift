// 枚举 macOS 屏幕窗口，输出 JSON。
// 用法:
//   winlist           仅当前屏幕上的窗口
//   winlist --all     包含最小化/离屏窗口
//
// 说明：macOS 15+ 已移除 CGWindowListCreateImage，因此截图交给
// /usr/sbin/screencapture -l <windowID>，这个工具只负责发现窗口。

import Foundation
import CoreGraphics

struct WindowInfo: Codable {
    let id: Int
    let owner: String
    let title: String
    let pid: Int
    let layer: Int
    let x: Double
    let y: Double
    let width: Double
    let height: Double
    let onscreen: Bool
}

func number(_ value: Any?) -> Double {
    (value as? NSNumber)?.doubleValue ?? 0
}

func integer(_ value: Any?) -> Int {
    (value as? NSNumber)?.intValue ?? 0
}

func string(_ value: Any?) -> String {
    value as? String ?? ""
}

let includeOffscreen = CommandLine.arguments.contains("--all")
let options: CGWindowListOption = includeOffscreen
    ? [.optionAll, .excludeDesktopElements]
    : [.optionOnScreenOnly, .excludeDesktopElements]

guard let raw = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
    FileHandle.standardError.write(Data("无法读取窗口列表\n".utf8))
    exit(1)
}

var result: [WindowInfo] = []
result.reserveCapacity(raw.count)

for item in raw {
    let bounds = item[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let owner = string(item[kCGWindowOwnerName as String])
    let title = string(item[kCGWindowName as String])
    let layer = integer(item[kCGWindowLayer as String])
    let onscreen: Bool
    if let flag = item[kCGWindowIsOnscreen as String] as? NSNumber {
        onscreen = flag.boolValue
    } else {
        onscreen = !includeOffscreen
    }

    result.append(WindowInfo(
        id: integer(item[kCGWindowNumber as String]),
        owner: owner,
        title: title,
        pid: integer(item[kCGWindowOwnerPID as String]),
        layer: layer,
        x: number(bounds["X"]),
        y: number(bounds["Y"]),
        width: number(bounds["Width"]),
        height: number(bounds["Height"]),
        onscreen: onscreen
    ))
}

let encoder = JSONEncoder()
encoder.outputFormatting = [.sortedKeys]
do {
    let data = try encoder.encode(result)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
} catch {
    FileHandle.standardError.write(Data("JSON 编码失败: \(error)\n".utf8))
    exit(1)
}
