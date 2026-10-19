//! Behaviour that must be the same on Windows, macOS, and Linux.

mod common;
use common::*;
use fontstrike::FontProject;
use fontstrike::theme::{
    Mode, Os, Settings, ThemePref, config_path_for, legacy_paths_for, parse_gnome_color_scheme,
    parse_gtk_theme, shortcut_label, system_theme_label,
};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, OsString> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), OsString::from(v)))
        .collect();
    move |k| map.get(k).cloned()
}

fn settings_in(base: &str, parts: &[&str]) -> PathBuf {
    let mut p = PathBuf::from(base);
    for part in parts {
        p.push(part);
    }
    p.join("FontStrike").join("settings.ini")
}

#[test]
fn settings_live_in_each_platforms_config_folder() {
    let win = env(&[
        ("APPDATA", r"C:\Users\Ana\AppData\Roaming"),
        ("USERPROFILE", r"C:\Users\Ana"),
    ]);
    assert_eq!(
        config_path_for(Os::Windows, &win),
        Some(settings_in(r"C:\Users\Ana\AppData\Roaming", &[]))
    );
    let win_no_appdata = env(&[("USERPROFILE", r"C:\Users\Ana")]);
    assert_eq!(
        config_path_for(Os::Windows, &win_no_appdata),
        Some(settings_in(r"C:\Users\Ana", &["AppData", "Roaming"]))
    );

    let mac = env(&[
        ("HOME", "/Users/ana"),
        ("XDG_CONFIG_HOME", "/Users/ana/.xdg"),
    ]);
    assert_eq!(
        config_path_for(Os::MacOs, &mac),
        Some(settings_in(
            "/Users/ana",
            &["Library", "Application Support"]
        )),
        "macOS uses Application Support, not XDG"
    );

    let linux = env(&[("HOME", "/home/ana"), ("XDG_CONFIG_HOME", "/home/ana/.cfg")]);
    assert_eq!(
        config_path_for(Os::Unix, &linux),
        Some(settings_in("/home/ana/.cfg", &[]))
    );
    let linux_default = env(&[("HOME", "/home/ana"), ("XDG_CONFIG_HOME", "")]);
    assert_eq!(
        config_path_for(Os::Unix, &linux_default),
        Some(settings_in("/home/ana", &[".config"]))
    );
    // Relative XDG paths are invalid per the spec and ignored.
    let linux_relative = env(&[("HOME", "/home/ana"), ("XDG_CONFIG_HOME", "rel/cfg")]);
    assert_eq!(
        config_path_for(Os::Unix, &linux_relative),
        Some(settings_in("/home/ana", &[".config"]))
    );
    // A stray APPDATA (e.g. under Wine) does not redirect Linux settings.
    let linux_appdata = env(&[("HOME", "/home/ana"), ("APPDATA", "/tmp/wine")]);
    assert_eq!(
        config_path_for(Os::Unix, &linux_appdata),
        Some(settings_in("/home/ana", &[".config"]))
    );
    assert_eq!(config_path_for(Os::Unix, &env(&[])), None);
}

#[test]
fn legacy_locations_are_only_the_ones_that_moved() {
    // Windows: earlier builds already used %APPDATA%, so nothing moves.
    let win = env(&[("APPDATA", r"C:\Users\Ana\AppData\Roaming")]);
    assert!(legacy_paths_for(Os::Windows, &win).is_empty());
    // macOS: earlier builds used ~/.config.
    let mac = env(&[("HOME", "/Users/ana")]);
    assert_eq!(
        legacy_paths_for(Os::MacOs, &mac),
        vec![settings_in("/Users/ana", &[".config"])]
    );
    // Linux: unchanged unless APPDATA had been set.
    let linux = env(&[("HOME", "/home/ana")]);
    assert!(legacy_paths_for(Os::Unix, &linux).is_empty());
    let linux_appdata = env(&[("HOME", "/home/ana"), ("APPDATA", "/tmp/wine")]);
    assert_eq!(
        legacy_paths_for(Os::Unix, &linux_appdata),
        vec![settings_in("/tmp/wine", &[])]
    );
}

#[test]
fn migration_copies_old_settings_and_never_deletes_them() {
    let dir = scratch("migrate");
    let old = dir.join("old").join("FontStrike").join("settings.ini");
    let new = dir.join("new").join("FontStrike").join("settings.ini");
    write(
        &old,
        "# FontStrike settings\ntheme = light\nfuture_key = 1\n",
    );
    let from = Settings::migrate(&new, std::slice::from_ref(&old)).unwrap();
    assert_eq!(from.as_deref(), Some(old.as_path()));
    assert!(old.is_file(), "the old file is kept");
    assert_eq!(fs::read(&old).unwrap(), fs::read(&new).unwrap());
    assert_eq!(Settings::load(&new).theme, ThemePref::Light);

    // Once the new file exists it wins and is never overwritten.
    let mut s = Settings::load(&new);
    s.theme = ThemePref::System;
    s.save(&new).unwrap();
    assert_eq!(
        Settings::migrate(&new, std::slice::from_ref(&old)).unwrap(),
        None
    );
    assert_eq!(Settings::load(&new).theme, ThemePref::System);
    assert_eq!(Settings::load(&old).theme, ThemePref::Light);

    // Nothing to migrate is not an error.
    let other = dir.join("none").join("settings.ini");
    assert_eq!(
        Settings::migrate(&other, &[dir.join("missing.ini")]).unwrap(),
        None
    );
    assert!(!other.exists());
}

#[test]
fn shortcut_hints_and_theme_label_follow_the_os() {
    assert_eq!(shortcut_label("Ctrl+Shift+Z", Os::MacOs), "Cmd+Shift+Z");
    assert_eq!(
        shortcut_label("Save the UFO source (Ctrl+S)", Os::MacOs),
        "Save the UFO source (Cmd+S)"
    );
    assert_eq!(shortcut_label("Ctrl+Shift+Z", Os::Windows), "Ctrl+Shift+Z");
    assert_eq!(shortcut_label("Ctrl+Q", Os::Unix), "Ctrl+Q");
    assert!(system_theme_label(Os::Windows).contains("Windows"));
    assert!(system_theme_label(Os::MacOs).contains("macOS"));
    assert!(!system_theme_label(Os::Unix).contains("Windows"));
}

#[test]
fn linux_desktop_theme_is_read_from_gnome_or_gtk() {
    assert_eq!(
        parse_gnome_color_scheme("'prefer-dark'\n"),
        Some(Mode::Dark)
    );
    assert_eq!(
        parse_gnome_color_scheme("'prefer-light'\n"),
        Some(Mode::Light)
    );
    assert_eq!(parse_gnome_color_scheme("'default'\n"), None);
    assert_eq!(parse_gnome_color_scheme(""), None);
    assert_eq!(parse_gtk_theme("Adwaita:dark"), Some(Mode::Dark));
    assert_eq!(parse_gtk_theme("Yaru-dark"), Some(Mode::Dark));
    assert_eq!(parse_gtk_theme("Adwaita"), Some(Mode::Light));
    assert_eq!(parse_gtk_theme(""), None);
}

#[test]
fn notes_name_ufo_files_with_forward_slashes_on_every_os() {
    let dir = scratch("separators");
    let root = dir.join("Rich.ufo");
    rich_ufo(&root);
    let project = FontProject::open(&root).unwrap();
    let notes = project.notes().join("\n");
    assert!(notes.contains("glyphs/README-unknown.txt"), "{notes}");
    assert!(!notes.contains('\\'), "{notes}");
}

#[test]
fn saved_ufo_text_files_use_lf_line_endings_on_every_os() {
    let dir = scratch("eol");
    let root = dir.join("Eol.ufo");
    let mut project = fontstrike::demo::build(&root).unwrap();
    project.set_width("A", 650.0).unwrap();
    project.save().unwrap();
    for (path, bytes) in tree(&root) {
        let name = path.to_string_lossy().into_owned();
        if name.ends_with(".plist") || name.ends_with(".glif") || name.ends_with(".fea") {
            assert!(
                !bytes.windows(2).any(|w| w == b"\r\n"),
                "{name} contains CRLF"
            );
        }
    }
}

/// Creates a symlink, or returns false where the OS does not allow it
/// (Windows without Developer Mode or the symlink privilege).
fn try_symlink(target: &Path, link: &Path, dir: bool) -> bool {
    #[cfg(unix)]
    {
        let _ = dir;
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        let r = if dir {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        };
        match r {
            Ok(()) => true,
            Err(e) => {
                eprintln!("skipping symlink case: {e}");
                false
            }
        }
    }
}

#[test]
fn symlinks_inside_or_as_the_ufo_are_refused_without_changes() {
    let dir = scratch("symlink");
    let real = dir.join("Real.ufo");
    fontstrike::demo::build(&real).unwrap();
    let before = tree(&real);

    let link_root = dir.join("Link.ufo");
    if try_symlink(&real, &link_root, true) {
        let Err(error) = FontProject::open(&link_root) else {
            panic!("symlinked UFO opened");
        };
        assert!(error.to_string().contains("symlink"), "{error}");
        assert_eq!(tree(&real), before, "link target unchanged");
    }

    let outside = dir.join("outside.txt");
    fs::write(&outside, "secret").unwrap();
    if try_symlink(&outside, &real.join("com.example.link.txt"), false) {
        let Err(error) = FontProject::open(&real) else {
            panic!("UFO with an inner symlink opened");
        };
        assert!(error.to_string().contains("symlink"), "{error}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "secret");
    }
}
