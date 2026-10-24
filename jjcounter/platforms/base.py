"""平台抽象：窗口发现 + 采集流。

macOS、Windows 只在各自实现里处理系统差异；core 只依赖这些协议。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path
from typing import List, Optional, Sequence

from ..core.models import RawFrame, WindowInfo, WindowSelector


class PlatformError(RuntimeError):
    pass


class WindowError(PlatformError):
    pass


class CaptureError(PlatformError):
    pass


class WindowService(ABC):
    @abstractmethod
    def list_windows(self, include_off_screen: bool = False) -> List[WindowInfo]:
        raise NotImplementedError

    def find_window(self, selector: WindowSelector) -> WindowInfo:
        windows = self.list_windows(selector.include_off_screen)
        if selector.window_id is not None:
            for window in windows:
                if window.id == selector.window_id:
                    return window
            raise WindowError(f"找不到窗口 ID {selector.window_id}")

        title_key = selector.title.casefold().strip()
        app_key = selector.app.casefold().strip() if selector.app else None
        scored = []
        for window in windows:
            if window.width < selector.min_width or window.height < selector.min_height:
                continue
            if window.layer != 0:
                continue
            window_title = window.title.casefold().strip()
            window_app = window.app.casefold().strip()
            if app_key and app_key not in window_app:
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
            if app_key and window_app == app_key:
                score += 15.0
            if window.onscreen:
                score += 5.0
            score += min(window.area / 1_000_000.0, 5.0)
            scored.append((score, window.area, window))

        if not scored:
            raise WindowError(self._hints(windows, title_key, app_key, selector))
        scored.sort(key=lambda item: (item[0], item[1]), reverse=True)
        return scored[0][2]

    @staticmethod
    def _hints(
        windows: Sequence[WindowInfo],
        title_key: str,
        app_key: Optional[str],
        selector: WindowSelector,
    ) -> str:
        candidates = [
            w
            for w in windows
            if w.layer == 0
            and w.width >= selector.min_width
            and w.height >= selector.min_height
            and not w.title.startswith("Item-")
        ]
        candidates.sort(
            key=lambda w: (
                title_key in w.title.casefold(),
                app_key is not None and app_key in w.app.casefold(),
                w.area,
            ),
            reverse=True,
        )
        body = "\n".join(
            f"  #{w.id}  {w.app}  {w.title!r}  {w.size}" for w in candidates[:12]
        )
        return (
            f"没有找到标题匹配 {selector.title!r} 的窗口。\n"
            f"可用窗口：\n{body or '  （无）'}\n"
            "提示：运行 `list` 查看完整窗口列表。"
        )


class CaptureStream(ABC):
    """平台采集流。read 返回 BGR ndarray。"""

    latest_path: Optional[Path] = None
    self_paced: bool = False
    target_fps: float = 15.0
    backend_name: str = "unknown"

    @abstractmethod
    def start(self) -> None:
        raise NotImplementedError

    @abstractmethod
    def read(self, timeout: float = 2.0) -> RawFrame:
        raise NotImplementedError

    @abstractmethod
    def stop(self) -> None:
        raise NotImplementedError


class Platform(ABC):
    name: str = "unknown"

    @property
    @abstractmethod
    def window_service(self) -> WindowService:
        raise NotImplementedError

    @abstractmethod
    def create_capture_stream(
        self,
        window: WindowInfo,
        *,
        fps: float = 15.0,
        quality: float = 0.88,
        backend: str = "auto",
        out_dir: Path = Path("frames"),
    ) -> CaptureStream:
        raise NotImplementedError

    @abstractmethod
    def capture_once(self, window: WindowInfo, out_path: Path, fmt: str = "png") -> Path:
        raise NotImplementedError
