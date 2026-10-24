"""JJ 斗地主记牌器。"""

from .capture import (
    CaptureError,
    Frame,
    WindowCapture,
    capture_once,
    capture_window,
)
from .window import (
    DEFAULT_TITLE,
    WindowError,
    WindowInfo,
    find_window,
    list_windows,
)

__version__ = "0.1.0"

__all__ = [
    "CaptureError",
    "DEFAULT_TITLE",
    "Frame",
    "WindowCapture",
    "WindowError",
    "WindowInfo",
    "capture_once",
    "capture_window",
    "find_window",
    "list_windows",
]
