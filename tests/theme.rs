//! Theme preference persistence and resolution.

mod common;
use fontstrike::theme::{Mode, Settings, ThemePref, resolve};
use std::fs;

#[test]
fn theme_choice_persists_across_restarts() {
    let dir = common::scratch("theme");
    let path = dir.join("FontStrike").join("settings.ini");
    // First launch: no file, default is Dark.
    assert_eq!(Settings::load(&path).theme, ThemePref::Dark);
    for pref in [
        ThemePref::Light,
        ThemePref::System,
        ThemePref::Dark,
        ThemePref::Light,
    ] {
        let mut s = Settings::load(&path);
        s.theme = pref;
        s.save(&path).unwrap();
        // "Restart": a fresh load sees the saved choice.
        assert_eq!(Settings::load(&path).theme, pref);
    }
    assert!(
        !path.with_extension("tmp").exists(),
        "atomic save leaves no temp file"
    );
    assert!(fs::read_to_string(&path).unwrap().contains("theme = light"));
}

#[test]
fn bad_or_foreign_settings_never_block_startup_and_unknown_keys_survive() {
    let dir = common::scratch("theme-bad");
    let path = dir.join("settings.ini");
    fs::write(&path, b"\xff\xfe not utf8").unwrap();
    assert_eq!(Settings::load(&path).theme, ThemePref::Dark);
    fs::write(&path, "theme = neon\n").unwrap();
    assert_eq!(Settings::load(&path).theme, ThemePref::Dark);
    fs::write(&path, "future_key = 42\nTHEME=Light\n# a comment\n").unwrap();
    let mut s = Settings::load(&path);
    assert_eq!(s.theme, ThemePref::Light);
    s.theme = ThemePref::System;
    s.save(&path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("future_key = 42") && text.contains("# a comment"),
        "{text}"
    );
    assert_eq!(Settings::load(&path).theme, ThemePref::System);
}

#[test]
fn system_choice_follows_the_os_and_falls_back_to_dark() {
    assert_eq!(resolve(ThemePref::System, Some(Mode::Light)), Mode::Light);
    assert_eq!(resolve(ThemePref::System, Some(Mode::Dark)), Mode::Dark);
    assert_eq!(resolve(ThemePref::System, None), Mode::Dark);
    assert_eq!(resolve(ThemePref::Light, Some(Mode::Dark)), Mode::Light);
    assert_eq!(resolve(ThemePref::Dark, Some(Mode::Light)), Mode::Dark);
}
