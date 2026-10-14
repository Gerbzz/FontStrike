# FontStrike screenshot provenance

`assets/screenshots/fontstrike-glyph-editor.png` is an uncomposited 1320 × 790 screenshot from the retained **October 11** FontStrike Windows desktop executable's built-in `--capture` path. SHA-256: `4E453E8D4EC0EF7C76C821AA340BB9918C78DB3E7A8327A93B2F1C9F75A73606`. It is an earlier checkpoint image, not a capture of the newest local UI work.

The executable has SHA-256 `21CFEC488492F0E35B329E65FCB1528E6AB1CDCC966E7A58E834E9555CD8A1A2`. Its depfile points to the prepared publication checkpoint; all three inputs (`src/bin/desktop.rs`, `src/lib.rs`, `third_party/vectorcraft/path.rs`) exactly match the public FontStrike repository at this update. The public and binary-matched `desktop.rs` has SHA-256 `528EBA2366C407D28255C35C12F6A8A6803E297214ED79D471D77EF275A4D09D` (Git blob `b56ba3d1b8730fdf37c0dd8949306ba4bc307c2f`).

The newer local working file `2026-10-10/task-7/fontstrike/src/bin/desktop.rs`, modified October 12, has SHA-256 `2A5B7C5016A4A3D828F35DB26616997B0C6CA31084CD2B3725D91DAF68DFEF9E` (Git blob `2f2bae9cc15bc2cf2b4a72e02de71c10c643ef97`). Its visible source delta adds a small “by PWDS” label beside the FONTSTRIKE heading. Neither the retained October 11 screenshot binary nor the current public source includes that newer UI line. No newer matching executable was found in the inspected retained checkpoints; the alternate retained binary is also dated October 11 and has no depfile. The newer local source was not overwritten or built.

The app's `--demo` option created a local, self-authored UFO with A and V outlines and an A/V kerning pair; `--capture` rendered its glyph editor and exited. The screenshot shows an original fixture and a relative capture path, with no client font or personal file. The UFO fixture is not distributed with this repository.

This run verifies bounded startup, source display, and built-in capture for the earlier checkpoint only. It does **not** count as current-version app screenshot coverage. It did not exercise pointer editing, save/reopen, TTF export, accessibility, or a fresh build/test suite. Separate earlier synthetic export checks are described in the README and development notes.
