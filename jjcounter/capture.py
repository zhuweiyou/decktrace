"""窗口截图：单次截图与连续采集。

macOS 15+ 已废弃 CGWindowListCreateImage，这里使用系统自带的
/usr/sbin/screencapture -l <windowID> 抓取指定窗口。
窗口即使被其他窗口遮挡也能抓到自身内容，但窗口最小化时无法抓取。
"""

from __future__ import annotations

import os
import shutil
import subprocess
import time
import uuid
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Callable, Iterator, Optional

from PIL import Image

from .window import DEFAULT_TITLE, WindowInfo, WindowError, find_window

SCREENCAPTURE = "/usr/sbin/screencapture"


class CaptureError(RuntimeError):
    """截图失败。"""


@dataclass(frozen=True)
class Frame:
    """一帧有效的窗口截图。"""

    path: Path
    captured_at: datetime
    window: WindowInfo
    width: int
    height: int
    scale: float
    elapsed: float

    @property
    def size(self) -> str:
        return f"{self.width}x{self.height}"

    @property
    def age(self) -> float:
        return max((datetime.now().astimezone() - self.captured_at).total_seconds(), 0.0)

    def __str__(self) -> str:
        return (
            f"{self.captured_at:%H:%M:%S.%f}"[:-3]
            + f"  {self.size}  scale={self.scale:.2f}  {self.elapsed * 1000:.0f}ms  {self.path}"
        )


def _normalize_format(fmt: str) -> str:
    fmt = fmt.lower().lstrip(".")
    if fmt in ("jpg", "jpeg"):
        return "jpg"
    if fmt == "png":
        return "png"
    raise ValueError(f"不支持的图片格式：{fmt}（可选 png / jpg）")


def _frame_suffix(fmt: str) -> str:
    return ".jpg" if _normalize_format(fmt) == "jpg" else ".png"


def _check_screen_capture_permission(stderr: str) -> str:
    hint = (
        "请到 系统设置 → 隐私与安全性 → 屏幕与系统音频录制，"
        "允许终端 / iTerm / DeepSeek Harness 录屏，然后重启该应用。"
    )
    lowered = stderr.lower()
    if "permission" in lowered or "not authorized" in lowered or "denied" in lowered:
        return f"\n{hint}"
    return ""


def capture_window(
    window: WindowInfo,
    out_path: Path,
    fmt: str = "png",
    shadow: bool = False,
    timeout: float = 10.0,
) -> Frame:
    """抓取指定窗口并原子写入 out_path。"""

    fmt = _normalize_format(fmt)
    out_path = Path(out_path).expanduser().resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)

    # 扩展名跟随实际格式，避免出现“jpg 内容、png 后缀”
    if out_path.suffix.lower() not in (".png", ".jpg", ".jpeg"):
        out_path = out_path.with_suffix(_frame_suffix(fmt))
    elif fmt == "png" and out_path.suffix.lower() in (".jpg", ".jpeg"):
        out_path = out_path.with_suffix(".png")
    elif fmt == "jpg" and out_path.suffix.lower() == ".png":
        out_path = out_path.with_suffix(".jpg")

    # 注意：screencapture 无法写入以 "." 开头的隐藏文件，临时文件不要加前导点
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
            raise CaptureError(
                f"screencapture 超时（窗口 #{window.id}，>{timeout:g}s）"
            ) from exc
        except OSError as exc:
            raise CaptureError(f"无法执行 screencapture：{exc}") from exc
        elapsed = time.monotonic() - started

        stderr = (proc.stderr or "").strip()
        if proc.returncode != 0 or not tmp_path.exists():
            raise CaptureError(
                f"screencapture 失败（窗口 #{window.id}）："
                f"{stderr or f'退出码 {proc.returncode}'}"
                f"{_check_screen_capture_permission(stderr)}"
            )
        if tmp_path.stat().st_size < 1024:
            raise CaptureError(
                f"screencapture 输出为空（窗口 #{window.id}，"
                f"{tmp_path.stat().st_size} bytes）"
                f"{_check_screen_capture_permission(stderr)}"
            )

        # 校验图片完整性，避免把半截文件交出去
        try:
            with Image.open(tmp_path) as image:
                image.load()
                width, height = image.size
        except Exception as exc:
            raise CaptureError(f"截图不是有效图片：{exc}") from exc

        if width <= 0 or height <= 0:
            raise CaptureError(f"截图尺寸异常：{width}x{height}")

        captured_at = datetime.now().astimezone()
        os.replace(tmp_path, out_path)
    finally:
        if tmp_path.exists():
            try:
                tmp_path.unlink()
            except OSError:
                pass

    scale = width / window.width if window.width else 1.0
    return Frame(
        path=out_path,
        captured_at=captured_at,
        window=window,
        width=width,
        height=height,
        scale=scale,
        elapsed=elapsed,
    )


def capture_once(
    out_path: Path,
    title: str = DEFAULT_TITLE,
    owner: Optional[str] = None,
    window_id: Optional[int] = None,
    fmt: str = "png",
    shadow: bool = False,
    include_off_screen: bool = False,
) -> Frame:
    """查找窗口并截一张图。"""

    window = find_window(
        title=title,
        owner=owner,
        window_id=window_id,
        include_off_screen=include_off_screen,
    )
    return capture_window(window, out_path, fmt=fmt, shadow=shadow)


class WindowCapture:
    """持续采集同一个窗口。

    默认只保留 frames/latest.png，避免磁盘被 2MB/帧的图片塞满；
    save_all=True 时额外保留每一帧。
    """

    def __init__(
        self,
        title: str = DEFAULT_TITLE,
        owner: Optional[str] = None,
        window_id: Optional[int] = None,
        interval: float = 0.5,
        fmt: str = "png",
        output_dir: Path = Path("frames"),
        save_all: bool = False,
        include_off_screen: bool = False,
        rediscover_interval: float = 5.0,
        max_consecutive_errors: int = 20,
    ) -> None:
        if interval < 0.05:
            raise ValueError("interval 不能小于 0.05 秒")
        self.title = title
        self.owner = owner
        self.window_id = window_id
        self.interval = interval
        self.fmt = _normalize_format(fmt)
        self.output_dir = Path(output_dir).expanduser()
        self.save_all = save_all
        self.include_off_screen = include_off_screen
        self.rediscover_interval = rediscover_interval
        self.max_consecutive_errors = max_consecutive_errors

        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.latest_path = self.output_dir / f"latest{_frame_suffix(self.fmt)}"

        self.window: Optional[WindowInfo] = None
        self.frames = 0
        self.errors = 0
        self.consecutive_errors = 0
        self.started_at: Optional[float] = None
        self.last_frame: Optional[Frame] = None
        self._resolved_at = 0.0

    def resolve_window(self) -> WindowInfo:
        """立即查找（或刷新）目标窗口。"""

        return self._get_window()

    def _get_window(self) -> WindowInfo:
        now = time.monotonic()
        if self.window is None or (now - self._resolved_at) >= self.rediscover_interval:
            self.window = find_window(
                title=self.title,
                owner=self.owner,
                window_id=self.window_id,
                include_off_screen=self.include_off_screen,
            )
            self._resolved_at = now
        return self.window

    def _next_target(self) -> Path:
        if not self.save_all:
            return self.latest_path
        stamp = datetime.now().strftime("%Y%m%d_%H%M%S_%f")[:-3]
        return self.output_dir / f"jjddz_{stamp}{_frame_suffix(self.fmt)}"

    def _copy_latest(self, source: Path) -> None:
        """把带时间戳的帧同步到 latest，供识别端只盯一个文件。"""

        tmp = self.latest_path.with_name(
            f"{self.latest_path.stem}.{os.getpid()}.{uuid.uuid4().hex[:8]}.tmp"
            f"{self.latest_path.suffix}"
        )
        try:
            shutil.copyfile(source, tmp)
            os.replace(tmp, self.latest_path)
        finally:
            if tmp.exists():
                try:
                    tmp.unlink()
                except OSError:
                    pass

    def capture(self) -> Frame:
        window = self._get_window()
        target = self._next_target()
        frame = capture_window(window, target, fmt=self.fmt)
        if self.save_all:
            self._copy_latest(target)
        self.frames += 1
        self.consecutive_errors = 0
        self.last_frame = frame
        return frame

    def frames_iter(
        self,
        duration: Optional[float] = None,
        max_frames: Optional[int] = None,
        on_error: Optional[Callable[[Exception], None]] = None,
    ) -> Iterator[Frame]:
        """按固定间隔产出截图帧，直到超时/达到帧数上限。

        单次截图失败不会终止采集：会重新查找窗口并在下一轮重试；
        连续失败 max_consecutive_errors 次才抛出异常。
        """

        self.started_at = time.monotonic()
        deadline = self.started_at + duration if duration else None

        while True:
            if max_frames is not None and self.frames >= max_frames:
                return
            if deadline is not None and time.monotonic() >= deadline:
                return

            tick = time.monotonic()
            try:
                frame = self.capture()
            except (CaptureError, WindowError) as exc:
                self.errors += 1
                self.consecutive_errors += 1
                self.window = None
                self._resolved_at = 0.0
                if self.consecutive_errors >= self.max_consecutive_errors:
                    raise
                if on_error is not None:
                    on_error(exc)
            else:
                yield frame
            finally:
                elapsed = time.monotonic() - tick
                remaining = self.interval - elapsed
                if remaining > 0:
                    time.sleep(remaining)

    def stop(self) -> None:
        """预留：后续接入识别线程时可以在这里发停止信号。"""


def capture_loop(
    on_frame: Callable[[Frame], None],
    on_error: Optional[Callable[[Exception], None]] = None,
    duration: Optional[float] = None,
    max_frames: Optional[int] = None,
    **kwargs,
) -> WindowCapture:
    """便捷封装：创建 WindowCapture 并持续回调。"""

    capture = WindowCapture(**kwargs)
    for frame in capture.frames_iter(
        duration=duration, max_frames=max_frames, on_error=on_error
    ):
        on_frame(frame)
    return capture
