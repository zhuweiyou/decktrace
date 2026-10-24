"""JJ 斗地主记牌器 —— 截图命令行入口。

当前阶段只负责稳定截图：
    python3 -m jjcounter list     查看窗口
    python3 -m jjcounter shot     截一张图
    python3 -m jjcounter watch    持续截图
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import unicodedata
from datetime import datetime
from pathlib import Path
from typing import List

from .capture import CaptureError, WindowCapture, capture_once
from .window import DEFAULT_TITLE, WindowError, list_windows


def _display_width(text: str) -> int:
    return sum(
        2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
        for char in text
    )


def _pad(text: str, width: int) -> str:
    return text + " " * max(0, width - _display_width(text))


def _print_table(headers: List[str], rows: List[List[str]]) -> None:
    widths = [
        max(_display_width(header), *(_display_width(row[i]) for row in rows))
        for i, header in enumerate(headers)
    ] if rows else [_display_width(header) for header in headers]

    print("  ".join(_pad(header, widths[i]) for i, header in enumerate(headers)))
    print("  ".join("-" * widths[i] for i in range(len(headers))))
    for row in rows:
        print("  ".join(_pad(cell, widths[i]) for i, cell in enumerate(row)))


def cmd_list(args: argparse.Namespace) -> int:
    windows = list_windows(include_off_screen=args.all)
    windows = [
        window
        for window in windows
        if window.layer == 0
        and window.width >= 80
        and window.height >= 80
        and (args.all or window.onscreen)
    ]
    windows.sort(key=lambda window: (not window.onscreen, window.owner, window.title))

    if args.json:
        print(
            json.dumps(
                [
                    {
                        "id": window.id,
                        "owner": window.owner,
                        "title": window.title,
                        "pid": window.pid,
                        "x": window.x,
                        "y": window.y,
                        "width": window.width,
                        "height": window.height,
                        "onscreen": window.onscreen,
                    }
                    for window in windows
                ],
                ensure_ascii=False,
                indent=2,
            )
        )
        return 0

    if not windows:
        print("没有找到可见窗口。")
        return 0

    rows = []
    for window in windows:
        marker = "→" if window.title.strip() == DEFAULT_TITLE else " "
        rows.append(
            [
                marker,
                str(window.id),
                str(window.pid),
                window.owner,
                window.title or "(无标题)",
                window.size,
                window.position,
                "是" if window.onscreen else "否",
            ]
        )
    _print_table(
        ["", "ID", "PID", "应用", "标题", "尺寸", "位置", "在屏"],
        rows,
    )
    print(f"\n共 {len(windows)} 个窗口；→ 表示标题包含 {DEFAULT_TITLE!r}。")
    return 0


def _default_shot_path(fmt: str) -> Path:
    suffix = ".jpg" if fmt == "jpg" else ".png"
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    return Path("shots") / f"jjddz_{stamp}{suffix}"


def cmd_shot(args: argparse.Namespace) -> int:
    out_path = Path(args.out) if args.out else _default_shot_path(args.format)
    try:
        frame = capture_once(
            out_path=out_path,
            title=args.title,
            owner=args.owner,
            window_id=args.window_id,
            fmt=args.format,
            shadow=args.shadow,
            include_off_screen=args.include_offscreen,
        )
    except WindowError as exc:
        print(f"找不到窗口：{exc}", file=sys.stderr)
        return 2
    except (CaptureError, ValueError) as exc:
        print(f"截图失败：{exc}", file=sys.stderr)
        return 1

    if args.json:
        print(
            json.dumps(
                {
                    "path": str(frame.path),
                    "captured_at": frame.captured_at.isoformat(),
                    "width": frame.width,
                    "height": frame.height,
                    "scale": frame.scale,
                    "elapsed_ms": round(frame.elapsed * 1000, 1),
                    "window": {
                        "id": frame.window.id,
                        "owner": frame.window.owner,
                        "title": frame.window.title,
                        "x": frame.window.x,
                        "y": frame.window.y,
                        "width": frame.window.width,
                        "height": frame.window.height,
                    },
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 0

    print(f"窗口：{frame.window}")
    print(
        f"截图：{frame.path}  {frame.width}x{frame.height}  "
        f"scale={frame.scale:.2f}  耗时 {frame.elapsed * 1000:.0f}ms"
    )
    return 0


def cmd_watch(args: argparse.Namespace) -> int:
    try:
        capture = WindowCapture(
            title=args.title,
            owner=args.owner,
            window_id=args.window_id,
            interval=args.interval,
            fmt=args.format,
            output_dir=Path(args.out_dir),
            save_all=args.save_all,
            include_off_screen=args.include_offscreen,
        )
        window = capture.resolve_window()
    except (WindowError, ValueError) as exc:
        print(f"初始化失败：{exc}", file=sys.stderr)
        return 2

    print(f"目标窗口：{window}")
    print(
        f"输出目录：{capture.output_dir.resolve()}  "
        f"最新帧：{capture.latest_path.name}  "
        f"间隔：{capture.interval:g}s  "
        f"{'保留全部帧' if capture.save_all else '只保留最新帧'}"
    )
    print("按 Ctrl+C 停止。\n")

    last_status = 0.0
    last_error_print = 0.0

    def on_error(exc: Exception) -> None:
        nonlocal last_error_print
        now = time.monotonic()
        if capture.consecutive_errors == 1 or now - last_error_print >= 3.0:
            print(
                f"[{datetime.now():%H:%M:%S}] 截图失败（连续 "
                f"{capture.consecutive_errors} 次）：{exc}",
                file=sys.stderr,
            )
            last_error_print = now

    try:
        for frame in capture.frames_iter(
            duration=args.duration,
            max_frames=args.max_frames,
            on_error=on_error,
        ):
            now = time.monotonic()
            if args.verbose:
                print(f"[{frame.captured_at:%H:%M:%S}] 第 {capture.frames} 帧  {frame}")
                last_status = now
            elif capture.frames == 1 or now - last_status >= args.status_every:
                elapsed = max(now - (capture.started_at or now), 1e-6)
                fps = capture.frames / elapsed
                print(
                    f"[{datetime.now():%H:%M:%S}] 已采集 {capture.frames} 帧  "
                    f"失败 {capture.errors} 次  {fps:.1f} fps  "
                    f"最新：{frame.size} {frame.elapsed * 1000:.0f}ms  "
                    f"→ {frame.path}"
                )
                last_status = now
    except KeyboardInterrupt:
        print("\n收到 Ctrl+C，停止采集。")
    except (WindowError, CaptureError) as exc:
        print(f"\n采集终止：{exc}", file=sys.stderr)
        return 2
    finally:
        elapsed = 0.0
        if capture.started_at is not None:
            elapsed = time.monotonic() - capture.started_at
        fps = capture.frames / elapsed if elapsed > 0 else 0.0
        print(
            f"\n采集结束：{capture.frames} 帧，失败 {capture.errors} 次，"
            f"用时 {elapsed:.1f}s，平均 {fps:.1f} fps"
        )
        if capture.last_frame:
            print(f"最新截图：{capture.last_frame.path.resolve()}")

    return 0


def cmd_run(args: argparse.Namespace) -> int:
    from .pipeline import run_counter

    color = (
        sys.stdout.isatty()
        and not args.no_color
        and os.environ.get("NO_COLOR") is None
    )
    return run_counter(
        title=args.title,
        owner=args.owner,
        window_id=args.window_id,
        interval=args.interval,
        min_hits=args.min_hits,
        auto_reset=not args.no_auto_reset,
        debug=args.debug,
        duration=args.duration,
        once=args.once,
        color=color,
        out_dir=Path(args.out_dir),
        capture_format=args.capture_format,
        capture_backend=args.capture_backend,
        sck_fps=args.sck_fps,
        sck_quality=args.sck_quality,
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python3 -m jjcounter",
        description="JJ 斗地主记牌器 · 截图采集",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    common = argparse.ArgumentParser(add_help=False)
    common.add_argument(
        "--title",
        default=DEFAULT_TITLE,
        help=f"窗口标题（默认：{DEFAULT_TITLE}）",
    )
    common.add_argument("--owner", default=None, help="所属应用名，如：微信")
    common.add_argument("--window-id", type=int, default=None, help="直接指定窗口 ID")
    common.add_argument(
        "--include-offscreen",
        action="store_true",
        help="包含最小化/离屏窗口",
    )

    list_parser = subparsers.add_parser("list", help="列出当前窗口")
    list_parser.add_argument("--all", action="store_true", help="包含离屏窗口")
    list_parser.add_argument("--json", action="store_true", help="输出 JSON")
    list_parser.set_defaults(func=cmd_list)

    shot_parser = subparsers.add_parser("shot", help="截取一张窗口截图", parents=[common])
    shot_parser.add_argument("--out", default=None, help="输出路径，默认 shots/jjddz_时间.png")
    shot_parser.add_argument("--format", choices=("png", "jpg"), default="png")
    shot_parser.add_argument("--shadow", action="store_true", help="保留窗口阴影（默认不保留）")
    shot_parser.add_argument("--json", action="store_true", help="输出 JSON")
    shot_parser.set_defaults(func=cmd_shot)

    watch_parser = subparsers.add_parser("watch", help="持续截取窗口", parents=[common])
    watch_parser.add_argument("--interval", type=float, default=0.5, help="采集间隔秒数，默认 0.5")
    watch_parser.add_argument("--duration", type=float, default=None, help="采集时长秒数，默认不限")
    watch_parser.add_argument("--max-frames", type=int, default=None, help="最多采集多少帧")
    watch_parser.add_argument("--out-dir", default="frames", help="输出目录，默认 frames")
    watch_parser.add_argument("--save-all", action="store_true", help="保留每一帧（默认只保留 latest）")
    watch_parser.add_argument("--format", choices=("png", "jpg"), default="png")
    watch_parser.add_argument("--status-every", type=float, default=5.0, help="状态行间隔秒数")
    watch_parser.add_argument("--verbose", action="store_true", help="每帧打印一行")
    watch_parser.set_defaults(func=cmd_watch)

    run_parser = subparsers.add_parser(
        "run", help="实时识别并显示终端记牌表", parents=[common]
    )
    run_parser.add_argument("--interval", type=float, default=0.15, help="采集间隔秒数，默认 0.15（约 5–6fps）")
    run_parser.add_argument(
        "--capture-format",
        choices=("jpg", "png"),
        default="jpg",
        help="legacy 截图格式，默认 jpg（更快）；需要无损调试时用 png",
    )
    run_parser.add_argument(
        "--capture-backend",
        choices=("sck", "legacy"),
        default="sck",
        help="sck=常驻 ScreenCaptureKit（默认），legacy=每帧 screencapture",
    )
    run_parser.add_argument("--sck-fps", type=float, default=15.0, help="常驻采集帧率，默认 15")
    run_parser.add_argument(
        "--sck-quality", type=float, default=0.88, help="常驻采集 JPEG 质量，默认 0.88"
    )
    run_parser.add_argument("--min-hits", type=int, default=2, help="同一张牌至少出现几帧才入账，默认 2")
    run_parser.add_argument("--no-auto-reset", action="store_true", help="关闭新一局自动重置")
    run_parser.add_argument("--debug", action="store_true", help="保存识别标注图到 frames/annotated.png")
    run_parser.add_argument("--duration", type=float, default=None, help="运行时长秒数，默认不限")
    run_parser.add_argument("--once", action="store_true", help="只抓一帧、打印一次表格后退出")
    run_parser.add_argument("--no-color", action="store_true", help="关闭终端颜色")
    run_parser.add_argument("--out-dir", default="frames", help="截图与标注图目录")
    run_parser.set_defaults(func=cmd_run)

    return parser


def main(argv: List[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
