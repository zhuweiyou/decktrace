from __future__ import annotations

import sys

from .base import Platform, PlatformError


def current_platform_name() -> str:
    if sys.platform == "darwin":
        return "macos"
    if sys.platform.startswith("win"):
        return "windows"
    return sys.platform


def create_platform(name: str | None = None) -> Platform:
    target = name or current_platform_name()
    if target == "macos":
        try:
            from .macos.platform import MacOSPlatform
        except Exception as exc:
            raise PlatformError(f"macOS 后端不可用：{exc}") from exc
        return MacOSPlatform()
    if target == "windows":
        try:
            from .windows.platform import WindowsPlatform
        except Exception as exc:
            raise PlatformError(
                "Windows 后端只能在 Windows 上运行，"
                "且截图后端尚未实现（架构已预留）"
            ) from exc
        return WindowsPlatform()
    raise PlatformError(f"暂不支持平台：{target}")
