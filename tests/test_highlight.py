import unittest

from decktrace.core.ui.cards import CardFaceFactory
from decktrace.core.ui.highlight import bomb_card_ids, card_visual_state
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

    def test_bottom_state_overrides_bomb(self):
        bomb = {"AH", "AS", "AC", "AD"}
        self.assertEqual(card_visual_state("AH", {"AH"}, bomb), "bottom")
        self.assertEqual(card_visual_state("AS", {"AH"}, bomb), "bomb")
        self.assertEqual(card_visual_state("2H", set(), set()), "normal")

    def test_bottom_tint_differs_from_normal_and_inverted(self):
        factory = CardFaceFactory(GAME)
        normal = factory.image("AH", 15, 30)
        bottom = factory.image("AH", 15, 30, bottom=True)
        inverted = factory.image("AH", 15, 30, invert=True)
        self.assertNotEqual(normal.tobytes(), bottom.tobytes())
        self.assertNotEqual(inverted.tobytes(), bottom.tobytes())


if __name__ == "__main__":
    unittest.main()
