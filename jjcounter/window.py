"""macOS 窗口发现与选择。

通过 tools/winlist.swift 编译出的小工具调用 CoreGraphics，
拿到窗口 ID、标题、所属进程和尺寸，供截图模块使用。
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional

PROJECT_ROOT = Path(__file__).resolve().parent.parent
TOOLS_DIR = PROJECT_ROOT / "tools"
BUILD_DIR = PROJECT_ROOT / ".build"
HELPER_SOURCE = TOOLS_DIR / "winlist.swift"
HELPER_BINARY = BUILD_DIR / "winlist"

DEFAULT_TITLE = "JJ斗地主"


class WindowError(RuntimeError):
    """窗口发现相关错误。"""


@dataclass(frozen=True)
class WindowInfo:
    id: int
    owner: str
    title: str
    pid: int
    layer: int
    x: float
    y: float
    width: float
    height: float
    onscreen: bool

    @property
    def area(self) -> float:
        return self.width * self.height

    @property
    def size(self) -> str:
        return f"{self.width:.0f}x{self.height:.0f}"

    @property
    def position(self) -> str:
        return f"({self.x:.0f}, {self.y:.0f})"

    def __str__(self) -> str:
        return (
            f"#{self.id} {self.owner} / {self.title!r} "
            f"{self.size} @ {self.position}"
        )


def _helper_is_fresh() -> bool:
    if not HELPER_BINARY.exists() or not os.access(HELPER_BINARY, os.X_OK):
        return False
    try:
        return HELPER_BINARY.stat().st_mtime >= HELPER_SOURCE.stat().st_mtime
    except OSError:
        return False


def ensure_helper(force: bool = False) -> Path:
    """确保窗口枚举工具已编译，返回可执行文件路径。"""

    if not force and _helper_is_fresh():
        return HELPER_BINARY

    swiftc = shutil.which("swiftc")
    if not swiftc:
        raise WindowError(
            "找不到 swiftc。请先安装 Xcode Command Line Tools：xcode-select --install"
        )
    if not HELPER_SOURCE.exists():
        raise WindowError(f"缺少窗口枚举源码：{HELPER_SOURCE}")

    BUILD_DIR.mkdir(parents=True, exist_ok=True)
    tmp = HELPER_BINARY.with_name(f"{HELPER_BINARY.name}.tmp")
    proc = subprocess.run(
        [swiftc, "-O", str(HELPER_SOURCE), "-o", str(tmp)],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0 or not tmp.exists():
        detail = (proc.stderr or proc.stdout or "未知错误").strip()
        raise WindowError(f"编译窗口枚举工具失败：\n{detail}")
    os.chmod(tmp, 0o755)
    os.replace(tmp, HELPER_BINARY)
    return HELPER_BINARY


def list_windows(include_off_screen: bool = False) -> List[WindowInfo]:
    """列出当前窗口，默认只列屏幕上可见的窗口。"""

    helper = ensure_helper()
    args = [str(helper)]
    if include_off_screen:
        args.append("--all")
    proc = subprocess.run(args, capture_output=True, text=True, timeout=10)
    if proc.returncode != 0:
        detail = (proc.stderr or proc.stdout or "未知错误").strip()
        raise WindowError(f"读取窗口列表失败：{detail}")
    try:
        rows = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise WindowError(f"窗口列表 JSON 解析失败：{exc}") from exc

    windows = [
        WindowInfo(
            id=int(row["id"]),
            owner=str(row.get("owner", "")),
            title=str(row.get("title", "")),
            pid=int(row.get("pid", 0)),
            layer=int(row.get("layer", 0)),
            x=float(row.get("x", 0)),
            y=float(row.get("y", 0)),
            width=float(row.get("width", 0)),
            height=float(row.get("height", 0)),
            onscreen=bool(row.get("onscreen", False)),
        )
        for row in rows
        if int(row.get("id", 0)) > 0
    ]
    return windows


def _normalize(value: str) -> str:
    return value.casefold().strip()


def find_window(
    title: str = DEFAULT_TITLE,
    owner: Optional[str] = None,
    window_id: Optional[int] = None,
    include_off_screen: bool = False,
    min_width: float = 120,
    min_height: float = 120,
) -> WindowInfo:
    """按窗口 ID / 标题 / 所属应用查找窗口，返回最匹配的一个。"""

    windows = list_windows(include_off_screen=include_off_screen)

    if window_id is not None:
        for window in windows:
            if window.id == window_id:
                return window
        raise WindowError(
            f"找不到窗口 ID {window_id}。窗口可能已关闭，"
            f"或它当前不可见（可加 --include-offscreen 再试）。"
        )

    title_key = _normalize(title)
    owner_key = _normalize(owner) if owner else None
    scored = []

    for window in windows:
        if window.width < min_width or window.height < min_height:
            continue
        if window.layer != 0:
            continue

        window_title = _normalize(window.title)
        window_owner = _normalize(window.owner)

        if owner_key and owner_key not in window_owner:
            continue

        if title_key:
            if window_title == title_key:
                score = 100.0
            elif window_title.startswith(title_key):
                score = 80.0
            elif title_key in window_title:
                score = 60.0
            elif window_title and window_title in title_key:
                score = 30.0
            else:
                continue
        else:
            score = 50.0

        if owner_key and window_owner == owner_key:
            score += 15.0
        if window.onscreen:
            score += 5.0
        if window.layer == 0:
            score += 5.0
        # 同分时略微偏向大窗口，避免选中同名的悬浮小窗
        score += min(window.area / 1_000_000.0, 5.0)

        scored.append((score, window.area, window))

    if not scored:
        hints = _candidate_hints(windows, title_key, owner_key)
        raise WindowError(
            f"没有找到标题匹配 {title!r} 的窗口。\n"
            f"可用窗口：\n{hints}\n"
            f"提示：可运行 `python3 -m jjcounter list` 查看完整窗口列表。"
        )

    scored.sort(key=lambda item: (item[0], item[1]), reverse=True)
    return scored[0][2]


def _candidate_hints(
    windows: List[WindowInfo], title_key: str, owner_key: Optional[str]
) -> str:
    candidates = [
        window
        for window in windows
        if window.layer == 0
        and not window.title.startswith("Item-")
        and window.width >= 120
        and window.height >= 120
    ]
    # 优先展示相关进程/标题
    candidates.sort(
        key=lambda window: (
            title_key in _normalize(window.title),
            owner_key is not None and owner_key in _normalize(window.owner),
            window.area,
        ),
        reverse=True,
    )
    lines = [
        f"  #{window.id}  {window.owner}  {window.title!r}  {window.size}"
        for window in candidates[:12]
    ]
    return "\n".join(lines) if lines else "  （无）"
