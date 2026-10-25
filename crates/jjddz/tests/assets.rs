use jjddz::profile;

#[test]
fn all_card_assets_and_templates_exist() {
    let game = profile();
    game.validate().expect("valid profile");

    assert!(game.templates_dir.join("ranks").is_dir());
    assert!(game.templates_dir.join("suits").is_dir());
    assert!(game.templates_dir.join("joker.png").exists());
    assert!(game.templates_dir.join("joker_compact.png").exists());
}
