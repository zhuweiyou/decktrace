"""常驻 ScreenCaptureKit 采集。

不再每帧启动 `screencapture`。Swift helper 启动一次，通过 stdout
发送带长度前缀的 JPEG 帧；Python 读帧直接解码，不经过磁盘。

帧协议（小端）:
    magic 4s = b"SCK1"
    seq   Q
    ts    d  (Unix 秒)
    width I
    height I
    size  I
    jpeg bytes
"""

from __future__ import annotations

import os
import select
import shutil
import struct
import subprocess
import threading
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Deque, Optional

from .window import BUILD_DIR, PROJECT_ROOT, WindowInfo

SCK_SOURCE = PROJECT_ROOT / "tools" / "sckcap.swift"
SCK_BINARY = BUILD_DIR / "sckcap"
FRAME_HEADER = struct.Struct("<4sQdIII")
MAX_JPEG_BYTES = 16 * 1024 * 1024


class SCKError(RuntimeError):
    """常驻采集启动/读取失败。"""


@dataclass(frozen=True)
class RawFrame:
    seq: int
    timestamp: float
    width: int
    height: int
    jpeg: bytes


def _helper_is_fresh() -> bool:
    if not SCK_BINARY.exists() or not os.access(SCK_BINARY, os.X_OK):
        return False
    try:
        return SCK_BINARY.stat().st_mtime >= SCK_SOURCE.stat().st_mtime
    except OSError:
        return False


def ensure_sck_helper(force: bool = False) -> Path:
    if not force and _helper_is_fresh():
        return SCK_BINARY
    swiftc = shutil.which("swiftc")
    if not swiftc:
        raise SCKError("找不到 swiftc，无法编译 ScreenCaptureKit 采集器")
    if not SCK_SOURCE.exists():
        raise SCKError(f"缺少采集器源码：{SCK_SOURCE}")

    BUILD_DIR.mkdir(parents=True, exist_ok=True)
    tmp = SCK_BINARY.with_name(f"{SCK_BINARY.name}.tmp")
    proc = subprocess.run(
        [swiftc, "-O", "-parse-as-library", str(SCK_SOURCE), "-o", str(tmp)],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0 or not tmp.exists():
        detail = (proc.stderr or proc.stdout or "未知错误").strip()
        raise SCKError(f"编译 ScreenCaptureKit 采集器失败：\n{detail}")
    os.chmod(tmp, 0o755)
    os.replace(tmp, SCK_BINARY)
    return SCK_BINARY


class SCKCapture:
    def __init__(
        self,
        window: WindowInfo,
        *,
        fps: float = 15.0,
        quality: float = 0.88,
        timeout: float = 5.0,
    ) -> None:
        self.window = window
        self.fps = fps
        self.quality = quality
        self.timeout = timeout
        self.proc: Optional[subprocess.Popen] = None
        self.helper_fps = 0.0
        self.stderr_tail: Deque[str] = deque(maxlen=40)
        self._first: Optional[RawFrame] = None
        self._stderr_thread: Optional[threading.Thread] = None
        self._stop = threading.Event()

    def start(self) -> None:
        helper = ensure_sck_helper()
        cmd = [
            str(helper),
            "--window-id",
            str(self.window.id),
            "--fps",
            f"{self.fps:g}",
            "--quality",
            f"{self.quality:g}",
            "--stream",
        ]
        self.proc = subprocess.Popen(
            cmd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        self._stderr_thread = threading.Thread(
            target=self._drain_stderr, name="sck-stderr", daemon=True
        )
        self._stderr_thread.start()

        try:
            self._first = self._read_raw(timeout=self.timeout)
        except Exception:
            self.stop()
            raise

    def _drain_stderr(self) -> None:
        assert self.proc is not None and self.proc.stderr is not None
        for raw in iter(self.proc.stderr.readline, b""):
            try:
                line = raw.decode(errors="replace").rstrip()
            except Exception:
                line = repr(raw)
            if not line:
                continue
            self.stderr_tail.append(line)
            if "fps=" in line:
                try:
                    self.helper_fps = float(line.split("fps=")[1].split()[0])
                except (ValueError, IndexError):
                    pass

    def _read_exact(self, size: int) -> bytes:
        assert self.proc is not None and self.proc.stdout is not None
        chunks = []
        remaining = size
        while remaining > 0:
            chunk = self.proc.stdout.read(remaining)
            if not chunk:
                raise SCKError(
                    f"采集器已退出（code={self.proc.poll()}）："
                    + " | ".join(list(self.stderr_tail)[-3:])
                )
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    def _read_raw(self, timeout: float = 2.0) -> RawFrame:
        assert self.proc is not None and self.proc.stdout is not None
        if self._stop.is_set():
            raise SCKError("采集已停止")
        ready, _, _ = select.select([self.proc.stdout], [], [], timeout)
        if not ready:
            if self.proc.poll() is not None:
                raise SCKError(
                    f"采集器已退出（code={self.proc.returncode}）："
                    + " | ".join(list(self.stderr_tail)[-3:])
                )
            raise SCKError(f"等待画面超时（{timeout:g}s）")
        header = self._read_exact(FRAME_HEADER.size)
        magic, seq, ts, width, height, size = FRAME_HEADER.unpack(header)
        if magic != b"SCK1":
            raise SCKError(f"帧头错误：{magic!r}")
        if size <= 0 or size > MAX_JPEG_BYTES:
            raise SCKError(f"帧长度异常：{size}")
        jpeg = self._read_exact(size)
        return RawFrame(seq, ts, width, height, jpeg)

    def take_first(self) -> Optional[RawFrame]:
        first, self._first = self._first, None
        return first

    def read(self, timeout: float = 2.0) -> RawFrame:
        return self._read_raw(timeout=timeout)

    def stop(self) -> None:
        self._stop.set()
        proc, self.proc = self.proc, None
        if proc is None:
            return
        try:
            proc.terminate()
            proc.wait(timeout=1.5)
        except Exception:
            try:
                proc.kill()
            except Exception:
                pass
        if self._stderr_thread is not None:
            self._stderr_thread.join(timeout=1.0)
            self._stderr_thread = None


def save_jpeg_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f"{path.stem}.{os.getpid()}.tmp{path.suffix}")
    tmp.write_bytes(data)
    os.replace(tmp, path)
