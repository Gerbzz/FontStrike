# FontStrike screenshot provenance

`assets/screenshots/fontstrike-glyph-editor.png` is an uncomposited 1320 × 790 screenshot from the retained FontStrike Windows desktop executable's built-in `--capture` path. SHA-256: `4E453E8D4EC0EF7C76C821AA340BB9918C78DB3E7A8327A93B2F1C9F75A73606`.

The executable has SHA-256 `21CFEC488492F0E35B329E65FCB1528E6AB1CDCC966E7A58E834E9555CD8A1A2`. All three source files named in its depfile (`src/bin/desktop.rs`, `src/lib.rs`, `third_party/vectorcraft/path.rs`) exactly match the public FontStrike repository at this update. The app's `--demo` option created a local, self-authored UFO with A and V outlines and an A/V kerning pair; `--capture` rendered its glyph editor and exited. The screenshot shows an original fixture and a relative capture path, with no client font or personal file. The UFO fixture is not distributed with this repository.

This run verifies a bounded startup, source display, and built-in capture. It did not exercise pointer editing, save/reopen, TTF export, accessibility, or a fresh build/test suite. Separate earlier synthetic export checks are described in the README and development notes.
