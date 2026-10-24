"""macOS 传统截图：每帧调用 /usr/sbin/screencapture。"""

from __future__ import annotations

import os
import subprocess
import time
import uuid
from datetime import datetime
from pathlib import Path
from typing import Optional

from PIL import Image

from ...core.models import Frame, WindowInfo
from ..base import CaptureError

SCREENCAPTURE = "/usr/sbin/screencapture"


def _normalize_format(fmt: str) -> str:
    fmt = fmt.lower().lstrip(".")
    if fmt in ("jpg", "jpeg"):
        return "jpg"
    if fmt == "png":
        return "png"
    raise ValueError(f"不支持的图片格式：{fmt}（可选 png / jpg）")


def capture_window(
    window: WindowInfo,
    out_path: Path,
    fmt: str = "png",
    shadow: bool = False,
    timeout: float = 10.0,
) -> Frame:
    fmt = _normalize_format(fmt)
    out_path = Path(out_path).expanduser().resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)

    if out_path.suffix.lower() not in (".png", ".jpg", ".jpeg"):
        out_path = out_path.with_suffix(".jpg" if fmt == "jpg" else ".png")
    elif fmt == "png" and out_path.suffix.lower() in (".jpg", ".jpeg"):
        out_path = out_path.with_suffix(".png")
    elif fmt == "jpg" and out_path.suffix.lower() == ".png":
        out_path = out_path.with_suffix(".jpg")

    # screencapture 不能写以 "." 开头的隐藏临时文件
    tmp_path = out_path.with_name(
        f"{out_path.stem}.{os.getpid()}.{uuid.uuid4().hex[:8]}.tmp{out_path.suffix}"
    )
    cmd = [SCREENCAPTURE, "-x", "-l", str(window.id), "-t", fmt]
    if not shadow:
        cmd.append("-o")
    cmd.append(str(tmp_path))

    started = time.monotonic()
    try:
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            raise CaptureError(f"screencapture 超时（窗口 #{window.id}）") from exc
        except OSError as exc:
            raise CaptureError(f"无法执行 screencapture：{exc}") from exc
        elapsed = time.monotonic() - started
        stderr = (proc.stderr or "").strip()
        if proc.returncode != 0 or not tmp_path.exists():
            raise CaptureError(
                f"screencapture 失败（窗口 #{window.id}）：{stderr or proc.returncode}"
            )
        if tmp_path.stat().st_size < 1024:
            raise CaptureError(f"screencapture 输出为空（窗口 #{window.id}）")
        try:
            with Image.open(tmp_path) as image:
                image.load()
                width, height = image.size
        except Exception as exc:
            raise CaptureError(f"截图不是有效图片：{exc}") from exc
        captured_at = datetime.now().astimezone()
        os.replace(tmp_path, out_path)
    finally:
        if tmp_path.exists():
            try:
                tmp_path.unlink()
            except OSError:
                pass

    return Frame(
        path=out_path,
        captured_at=captured_at,
        window=window,
        width=width,
        height=height,
        scale=width / window.width if window.width else 1.0,
        elapsed=elapsed,
    )


def capture_once(
    window: WindowInfo,
    out_path: Path,
    fmt: str = "png",
    shadow: bool = False,
) -> Frame:
    return capture_window(window, out_path, fmt=fmt, shadow=shadow)
