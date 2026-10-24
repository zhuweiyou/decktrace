"""macOS 窗口发现：调用 native/winlist.swift 的编译产物。"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import List

from ...core.models import WindowInfo
from ..base import WindowError, WindowService

PROJECT_ROOT = Path(__file__).resolve().parents[3]
BUILD_DIR = PROJECT_ROOT / ".build"
NATIVE_DIR = Path(__file__).resolve().parent / "native"
HELPER_SOURCE = NATIVE_DIR / "winlist.swift"
HELPER_BINARY = BUILD_DIR / "winlist"


def _helper_is_fresh() -> bool:
    if not HELPER_BINARY.exists() or not os.access(HELPER_BINARY, os.X_OK):
        return False
    try:
        return HELPER_BINARY.stat().st_mtime >= HELPER_SOURCE.stat().st_mtime
    except OSError:
        return False


def ensure_window_helper(force: bool = False) -> Path:
    if not force and _helper_is_fresh():
        return HELPER_BINARY
    swiftc = shutil.which("swiftc")
    if not swiftc:
        raise WindowError("找不到 swiftc，请安装 Xcode Command Line Tools")
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


class MacOSWindowService(WindowService):
    def list_windows(self, include_off_screen: bool = False) -> List[WindowInfo]:
        helper = ensure_window_helper()
        args = [str(helper)] + (["--all"] if include_off_screen else [])
        proc = subprocess.run(args, capture_output=True, text=True, timeout=10)
        if proc.returncode != 0:
            raise WindowError(
                f"读取窗口列表失败：{(proc.stderr or proc.stdout or '').strip()}"
            )
        try:
            rows = json.loads(proc.stdout)
        except json.JSONDecodeError as exc:
            raise WindowError(f"窗口列表 JSON 解析失败：{exc}") from exc

        result = []
        for row in rows:
            if int(row.get("id", 0)) <= 0:
                continue
            result.append(
                WindowInfo(
                    id=int(row["id"]),
                    app=str(row.get("owner", "")),
                    title=str(row.get("title", "")),
                    pid=int(row.get("pid", 0)),
                    x=float(row.get("x", 0)),
                    y=float(row.get("y", 0)),
                    width=float(row.get("width", 0)),
                    height=float(row.get("height", 0)),
                    onscreen=bool(row.get("onscreen", False)),
                    layer=int(row.get("layer", 0)),
                )
            )
        return result
