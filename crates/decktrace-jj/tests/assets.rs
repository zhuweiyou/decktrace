use decktrace_jj::profile;

#[test]
fn all_card_assets_and_templates_exist() {
    let game = profile();
    game.validate().expect("valid profile");

    for card_id in game.all_card_ids() {
        let path = game.card_assets_dir.join(format!("{card_id}.png"));
        assert!(path.exists(), "缺少牌面素材：{}", path.display());
    }
    assert!(game.templates_dir.join("ranks").is_dir());
    assert!(game.templates_dir.join("suits").is_dir());
    assert!(game.templates_dir.join("joker.png").exists());
    assert!(game.templates_dir.join("joker_compact.png").exists());
}
