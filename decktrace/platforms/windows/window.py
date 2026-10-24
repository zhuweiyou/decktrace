"""Windows 窗口发现（实验性，尚未在 Windows 实机验证）。

使用 Win32 ctypes 枚举顶层窗口；截图后端暂未实现，见 capture.py。
"""

from __future__ import annotations

import ctypes
import os
from ctypes import wintypes
from typing import List

from ...core.models import WindowInfo
from ..base import WindowError, WindowService

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

EnumWindowsProc = ctypes.WINFUNCTYPE(
    wintypes.BOOL, wintypes.HWND, wintypes.LPARAM
)

try:
    user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
except Exception:
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        pass


class WindowsWindowService(WindowService):
    def _process_path(self, pid: int) -> str:
        PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
        handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
        if not handle:
            return ""
        try:
            size = wintypes.DWORD(1024)
            buf = ctypes.create_unicode_buffer(size.value)
            if kernel32.QueryFullProcessImageNameW(
                handle, 0, buf, ctypes.byref(size)
            ):
                return buf.value
            return ""
        finally:
            kernel32.CloseHandle(handle)

    def list_windows(self, include_off_screen: bool = False) -> List[WindowInfo]:
        result: List[WindowInfo] = []

        def callback(hwnd, _lparam):
            if not user32.IsWindow(hwnd):
                return True
            length = user32.GetWindowTextLengthW(hwnd)
            if length <= 0:
                return True
            title_buf = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, title_buf, length + 1)
            title = title_buf.value.strip()
            if not title:
                return True

            rect = wintypes.RECT()
            if not user32.GetWindowRect(hwnd, ctypes.byref(rect)):
                return True
            width = max(0, rect.right - rect.left)
            height = max(0, rect.bottom - rect.top)
            if width < 80 or height < 80:
                return True

            visible = bool(user32.IsWindowVisible(hwnd))
            minimized = bool(user32.IsIconic(hwnd))
            if not include_off_screen and (not visible or minimized):
                return True

            pid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            path = self._process_path(int(pid.value))
            app = os.path.basename(path) if path else ""
            result.append(
                WindowInfo(
                    id=int(hwnd),
                    title=title,
                    app=app,
                    pid=int(pid.value),
                    x=float(rect.left),
                    y=float(rect.top),
                    width=float(width),
                    height=float(height),
                    onscreen=bool(visible and not minimized),
                    layer=0,
                )
            )
            return True

        user32.EnumWindows(EnumWindowsProc(callback), 0)
        return result
