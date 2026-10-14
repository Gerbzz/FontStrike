# FontStrike screenshot provenance

`assets/screenshots/fontstrike-glyph-editor.png` is an uncomposited 1320 × 790 image returned by the FontStrike Windows desktop app's built-in `--capture` command on October 14, 2026. SHA-256: `A13636E3C8667258593A74372D61194C672AB7A3AA02E1527094012F643A4D08`.

The app was built from this repository's source with `cargo build --offline --locked --features desktop --bin fontstrike-desktop -j 1`, using an existing local Cargo target cache. The build succeeded in 12.19 seconds. The resulting executable has SHA-256 `76B673A87201289BAF2A49EDAA943A16F5E433674779B73BE5996EECA25C4DDA`. Its depfile names three source inputs in this repository: `src/bin/desktop.rs`, `src/lib.rs`, and `third_party/vectorcraft/path.rs`. The desktop source has SHA-256 `2A5B7C5016A4A3D828F35DB26616997B0C6CA31084CD2B3725D91DAF68DFEF9E` and includes the "by PWDS" toolbar label.

The capture opened a self-authored local UFO sample through `--project capture\StrikeSans.ufo` and saved the frame with `--capture capture\glyph-editor-2026-10-14.png`. The sample contains A and V outlines and an A/V kerning pair. Relative paths were used so the image contains no personal workstation path. The fixture is not distributed with this repository.

The app exited successfully, and the image was visually inspected. This checks startup, sample rendering, and the app's capture command. It does not establish pointer editing, save/reopen, TTF export, accessibility, or installer readiness. Earlier synthetic export checks are described in the README and development notes. The previous October 11 capture remains in repository history; this image supersedes it for the current source.
