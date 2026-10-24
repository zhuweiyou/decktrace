"""游戏注册表：换游戏只注册新的 GameProfile。"""

from __future__ import annotations

from typing import Dict, Iterable, List

from .base import GameProfile
from .jj.profile import JJ_GAME

_GAMES: Dict[str, GameProfile] = {}


def register_game(game: GameProfile) -> GameProfile:
    game.validate()
    _GAMES[game.id] = game
    return game


def get_game(game_id: str = "jj") -> GameProfile:
    if game_id not in _GAMES:
        raise KeyError(f"未知游戏：{game_id}，可用：{', '.join(_GAMES)}")
    return _GAMES[game_id]


def list_games() -> List[GameProfile]:
    return list(_GAMES.values())


register_game(JJ_GAME)
