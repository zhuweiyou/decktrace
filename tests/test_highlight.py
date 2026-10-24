import unittest

from decktrace.core.ui.highlight import bomb_card_ids
from decktrace.games import get_game

GAME = get_game("jj")


class BombHighlightTest(unittest.TestCase):
    def test_full_deck_highlights_all(self):
        remaining = GAME.all_card_ids()
        bombs = bomb_card_ids(remaining, GAME)
        self.assertEqual(len(bombs), 54)

    def test_missing_one_card_breaks_bomb(self):
        remaining = [c for c in GAME.all_card_ids() if c != "AH"]
        bombs = bomb_card_ids(remaining, GAME)
        self.assertNotIn("AS", bombs)
        self.assertNotIn("AH", bombs)

    def test_single_joker_no_rocket(self):
        remaining = [c for c in GAME.all_card_ids() if c not in ("BJ", "SJ")]
        remaining = list(remaining) + ["SJ"]
        bombs = bomb_card_ids(remaining, GAME)
        self.assertNotIn("SJ", bombs)
        self.assertNotIn("BJ", bombs)

    def test_rocket_highlighted(self):
        remaining = ["BJ", "SJ", "2H"]
        bombs = bomb_card_ids(remaining, GAME)
        self.assertEqual(bombs, {"BJ", "SJ"})


if __name__ == "__main__":
    unittest.main()
