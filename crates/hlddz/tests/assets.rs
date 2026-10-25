use hlddz::profile;

#[test]
fn assets_are_complete() {
    let game = profile();
    game.validate().expect("profile");

    let ranks = game.templates_dir.join("ranks");
    let suits = game.templates_dir.join("suits");
    assert!(ranks.is_dir());
    assert!(suits.is_dir());

    for rank in &game.display_ranks {
        if game.labels.contains_key(rank) {
            continue; // 大小王走 joker 模板
        }
        let path = ranks.join(format!("{rank}.png"));
        assert!(path.exists(), "缺少点数模板 {}", path.display());
    }
    for suit in &game.suits {
        let path = suits.join(format!("{suit}.png"));
        assert!(path.exists(), "缺少花色模板 {}", path.display());
    }
}
