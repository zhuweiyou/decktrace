import unittest
from pathlib import Path

from decktrace.core.models import WindowInfo, WindowSelector
from decktrace.games import get_game, list_games
from decktrace.platforms.base import WindowService, WindowError


class FakeWindowService(WindowService):
    def __init__(self, windows):
        self.windows = windows

    def list_windows(self, include_off_screen=False):
        if include_off_screen:
            return list(self.windows)
        return [w for w in self.windows if w.onscreen]


class FakeSelectorTest(unittest.TestCase):
    def setUp(self):
        self.service = FakeWindowService(
            [
                WindowInfo(1, "斗地主", "微信", width=900, height=550),
                WindowInfo(2, "斗地主", "WeChat", width=1280, height=720),
                WindowInfo(3, "别的窗口", "微信", width=900, height=550),
            ]
        )

    def test_title_and_app_selection(self):
        window = self.service.find_window(
            WindowSelector(title="斗地主", app="微信")
        )
        self.assertEqual(window.id, 1)

    def test_exact_title_falls_back_to_any_app(self):
        window = self.service.find_window(WindowSelector(title="斗地主"))
        self.assertIn(window.id, (1, 2))

    def test_id_priority(self):
        window = self.service.find_window(
            WindowSelector(title="不存在的窗口", window_id=3)
        )
        self.assertEqual(window.id, 3)

    def test_not_found_raises(self):
        with self.assertRaises(WindowError):
            self.service.find_window(WindowSelector(title="不存在"))


class GameProfileTest(unittest.TestCase):
    def test_jj_profile(self):
        game = get_game("jj")
        self.assertEqual(game.id, "jj")
        self.assertEqual(game.total_cards, 54)
        self.assertEqual(set(game.display_ranks), set(game.rank_totals))
        self.assertTrue(game.templates_dir.exists())
        self.assertIn("jj", [g.id for g in list_games()])

    def test_labels(self):
        game = get_game("jj")
        self.assertEqual(game.label("BJ"), "大王")
        self.assertEqual(game.label("A", "H"), "A♥")


if __name__ == "__main__":
    unittest.main()
