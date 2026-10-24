"""命令行入口：平台 + 游戏 + 识别来源都是可替换的。"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import unicodedata
from dataclasses import replace
from datetime import datetime
from pathlib import Path
from typing import List, Optional

from . import APP_NAME
from .core.models import WindowSelector
from .games import get_game, list_games
from .platforms import CaptureError, PlatformError, WindowError, create_platform


def _display_width(text: str) -> int:
    return sum(
        2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1 for ch in text
    )


def _pad(text: str, width: int) -> str:
    return text + " " * max(0, width - _display_width(text))


def _print_table(headers: List[str], rows: List[List[str]]) -> None:
    widths = (
        [
            max(_display_width(header), *(_display_width(row[i]) for row in rows))
            for i, header in enumerate(headers)
        ]
        if rows
        else [_display_width(header) for header in headers]
    )
    print("  ".join(_pad(header, widths[i]) for i, header in enumerate(headers)))
    print("  ".join("-" * widths[i] for i in range(len(headers))))
    for row in rows:
        print("  ".join(_pad(cell, widths[i]) for i, cell in enumerate(row)))


def _selector(args: argparse.Namespace, game) -> WindowSelector:
    base = game.window
    return replace(
        base,
        title=args.title if args.title is not None else base.title,
        app=args.owner if args.owner is not None else base.app,
        window_id=args.window_id,
        include_off_screen=args.include_offscreen,
    )


def cmd_games(args: argparse.Namespace) -> int:
    for game in list_games():
        print(
            f"{game.id:8} {game.name}  窗口={game.window.title!r}  "
            f"牌数={game.total_cards}  来源={','.join(game.supported_sources)}  "
            f"模板={game.templates_dir}"
        )
    return 0


def cmd_list(args: argparse.Namespace) -> int:
    platform = create_platform(args.platform)
    game = get_game(args.game)
    windows = platform.window_service.list_windows(include_off_screen=args.all)
    windows = [
        window
        for window in windows
        if window.layer == 0
        and window.width >= 80
        and window.height >= 80
        and (args.all or window.onscreen)
    ]
    windows.sort(key=lambda window: (not window.onscreen, window.app, window.title))

    if args.json:
        print(
            json.dumps(
                [
                    {
                        "id": window.id,
                        "app": window.app,
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
        marker = "→" if window.title.strip() == game.window.title else " "
        rows.append(
            [
                marker,
                str(window.id),
                str(window.pid),
                window.app,
                window.title or "(无标题)",
                window.size,
                window.position,
                "是" if window.onscreen else "否",
            ]
        )
    _print_table(
        ["", "ID", "PID", "应用", "标题", "尺寸", "位置", "在屏"], rows
    )
    print(f"\n共 {len(windows)} 个窗口；→ 表示标题匹配游戏 {game.name!r}。")
    return 0


def cmd_shot(args: argparse.Namespace) -> int:
    platform = create_platform(args.platform)
    game = get_game(args.game)
    selector = _selector(args, game)
    try:
        window = platform.window_service.find_window(selector)
    except WindowError as exc:
        print(f"找不到窗口：{exc}", file=sys.stderr)
        return 2

    suffix = ".jpg" if args.format == "jpg" else ".png"
    out_path = (
        Path(args.out)
        if args.out
        else Path("shots") / f"{game.id}_{datetime.now():%Y%m%d_%H%M%S}{suffix}"
    )
    try:
        path = platform.capture_once(window, out_path, fmt=args.format)
    except (CaptureError, PlatformError) as exc:
        print(f"截图失败：{exc}", file=sys.stderr)
        return 1

    if args.json:
        print(
            json.dumps(
                {
                    "path": str(path),
                    "width": window.width,
                    "height": window.height,
                    "window": {
                        "id": window.id,
                        "app": window.app,
                        "title": window.title,
                    },
                },
                ensure_ascii=False,
                indent=2,
            )
        )
    else:
        print(f"窗口：{window}")
        print(f"截图：{path}")
    return 0


def cmd_watch(args: argparse.Namespace) -> int:
    import cv2

    platform = create_platform(args.platform)
    game = get_game(args.game)
    selector = _selector(args, game)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    try:
        window = platform.window_service.find_window(selector)
        stream = platform.create_capture_stream(
            window,
            fps=1.0 / max(args.interval, 0.02),
            backend=args.capture_backend,
            out_dir=out_dir,
        )
    except (WindowError, CaptureError, PlatformError) as exc:
        print(f"初始化失败：{exc}", file=sys.stderr)
        return 2

    print(f"窗口：{window}")
    print(f"采集后端：{stream.backend_name}   输出：{out_dir.resolve()}")
    print("按 Ctrl+C 停止。")
    suffix = ".jpg" if args.format == "jpg" else ".png"
    frames = 0
    started = time.monotonic()
    try:
        stream.start()
        while True:
            if args.duration is not None and time.monotonic() - started >= args.duration:
                break
            if args.max_frames is not None and frames >= args.max_frames:
                break
            raw = stream.read(timeout=2.0)
            frames += 1
            stamp = f"{datetime.now():%Y%m%d_%H%M%S_%f}"[:-3]
            path = out_dir / f"{game.id}_{stamp}{suffix}"
            if args.save_all:
                cv2.imwrite(str(path), raw.image)
            latest = out_dir / f"latest{suffix}"
            cv2.imwrite(str(latest), raw.image)
            if args.verbose:
                print(f"[{frames}] {raw.width}x{raw.height} {raw.timestamp:.3f}")
            if not stream.self_paced:
                time.sleep(args.interval)
    except KeyboardInterrupt:
        print("\n停止采集。")
    finally:
        stream.stop()
        elapsed = max(time.monotonic() - started, 1e-6)
        print(f"采集结束：{frames} 帧，{frames / elapsed:.1f} fps")
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    from .core.runtime import run_game
    from .core.ui.factory import create_renderer
    from .games.registry import get_game
    from .sources.factory import create_detection_source

    game = get_game(args.game)
    platform = create_platform(args.platform)
    selector = _selector(args, game)
    try:
        window = platform.window_service.find_window(selector)
    except WindowError as exc:
        print(f"找不到窗口：{exc}", file=sys.stderr)
        return 2

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    source_kind = args.source or game.default_source
    if source_kind not in game.supported_sources:
        print(
            f"{game.name} 不支持来源 {source_kind}，支持：{', '.join(game.supported_sources)}",
            file=sys.stderr,
        )
        return 2

    color = (
        sys.stdout.isatty()
        and not args.no_color
        and os.environ.get("NO_COLOR") is None
    )

    if args.once:
        import cv2
        from .core.deck import CardTracker
        from .core.table import render_table
        from .recognition import CardRecognizer, annotate

        try:
            path = platform.capture_once(window, out_dir / f"{game.id}_once.png")
            image = cv2.imread(str(path), cv2.IMREAD_COLOR)
            if image is None:
                raise CaptureError(f"无法读取截图：{path}")
            result = CardRecognizer(game).recognize(image)
            tracker = CardTracker(game, min_hits=1)
            # 单帧模式没有后续帧，喂两遍以便确认底牌后开局。
            tracker.observe(result.detections)
            tracker.observe(result.detections)
            print(render_table(tracker.snapshot(), game, color=color, status="单帧模式"))
            if args.debug:
                cv2.imwrite(str(out_dir / "annotated.png"), annotate(image, result.detections))
            return 0
        except (CaptureError, PlatformError) as exc:
            print(f"截图失败：{exc}", file=sys.stderr)
            return 1

    try:
        source = create_detection_source(
            source_kind,
            game=game,
            platform=platform,
            window=window,
            out_dir=out_dir,
            fps=args.capture_fps,
            quality=args.capture_quality,
            capture_backend=args.capture_backend,
            debug=args.debug,
        )
    except (PlatformError, CaptureError, OSError) as exc:
        print(f"初始化识别来源失败：{exc}", file=sys.stderr)
        return 2
    try:
        renderer = create_renderer(
            args.ui,
            game,
            color=color,
            topmost=not args.no_topmost,
            highlight_bombs=not args.no_highlight_bombs,
        )
    except Exception as exc:
        print(f"初始化 UI 失败：{exc}", file=sys.stderr)
        source.stop()
        return 2
    return run_game(
        game=game,
        source=source,
        out_dir=out_dir,
        auto_reset=not args.no_auto_reset,
        min_hits=args.min_hits,
        debug=args.debug,
        duration=args.duration,
        renderer=renderer,
        header=f"目标窗口：{window}",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="decktrace", description=APP_NAME)
    sub = parser.add_subparsers(dest="command", required=True)

    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--game", default="jj", help="游戏 profile，默认 jj")
    common.add_argument("--title", default=None, help="覆盖游戏窗口标题")
    common.add_argument("--owner", default=None, help="覆盖所属应用名")
    common.add_argument("--window-id", type=int, default=None, help="直接指定窗口 ID")
    common.add_argument("--include-offscreen", action="store_true", help="包含离屏窗口")
    common.add_argument("--platform", default=None, help="强制平台：macos/windows")

    games_p = sub.add_parser("games", help="列出已注册游戏")
    games_p.set_defaults(func=cmd_games)

    list_p = sub.add_parser("list", help="列出窗口", parents=[common])
    list_p.add_argument("--all", action="store_true", help="包含离屏窗口")
    list_p.add_argument("--json", action="store_true")
    list_p.set_defaults(func=cmd_list)

    shot_p = sub.add_parser("shot", help="截取一张窗口截图", parents=[common])
    shot_p.add_argument("--out", default=None)
    shot_p.add_argument("--format", choices=("png", "jpg"), default="png")
    shot_p.add_argument("--json", action="store_true")
    shot_p.set_defaults(func=cmd_shot)

    watch_p = sub.add_parser("watch", help="持续截图", parents=[common])
    watch_p.add_argument("--interval", type=float, default=0.5)
    watch_p.add_argument("--duration", type=float, default=None)
    watch_p.add_argument("--max-frames", type=int, default=None)
    watch_p.add_argument("--out-dir", default="frames")
    watch_p.add_argument("--save-all", action="store_true")
    watch_p.add_argument("--format", choices=("png", "jpg"), default="jpg")
    watch_p.add_argument("--capture-backend", choices=("auto", "sck", "legacy"), default="auto")
    watch_p.add_argument("--verbose", action="store_true")
    watch_p.set_defaults(func=cmd_watch)

    run_p = sub.add_parser("run", help="实时识别并显示记牌表", parents=[common])
    run_p.add_argument("--source", choices=("image", "memory"), default=None)
    run_p.add_argument("--capture-fps", "--sck-fps", dest="capture_fps", type=float, default=15.0)
    run_p.add_argument("--capture-quality", "--sck-quality", dest="capture_quality", type=float, default=0.88)
    run_p.add_argument("--capture-backend", choices=("auto", "sck", "legacy"), default="auto")
    run_p.add_argument("--min-hits", type=int, default=2)
    run_p.add_argument("--no-auto-reset", action="store_true")
    run_p.add_argument("--debug", action="store_true")
    run_p.add_argument("--duration", type=float, default=None)
    run_p.add_argument("--once", action="store_true")
    run_p.add_argument("--no-color", action="store_true")
    run_p.add_argument("--ui", choices=("tui", "gui"), default="gui", help="gui（默认，置顶牌面窗口）或 tui（调试/e2e，带 fps 等信息）")
    run_p.add_argument("--no-topmost", action="store_true", help="GUI 不置顶")
    run_p.add_argument(
        "--no-highlight-bombs", action="store_true", help="GUI 不反色高亮炸弹"
    )
    run_p.add_argument("--out-dir", default="frames")
    run_p.set_defaults(func=cmd_run)

    return parser


def main(argv: Optional[List[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except PlatformError as exc:
        print(f"平台不可用：{exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
