use std::path::PathBuf;

#[test]
fn gui_cards_are_complete() {
    let game = jjddz::profile();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/gui/cards");
    for card_id in game.all_card_ids() {
        let path = dir.join(format!("{card_id}.png"));
        assert!(path.exists(), "缺少 GUI 牌面素材：{}", path.display());
    }
}
