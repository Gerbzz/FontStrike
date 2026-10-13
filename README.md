# FontStrike

**by PWDS**

**Draw the shape of a letter, then keep it editable.** FontStrike is a small Windows type-design workspace for outlining glyphs, setting widths and kerning pairs, and exporting a TTF from a UFO source project.

[Try the source](#try-the-windows-preview) · [Detailed workflow and verification](docs/DEVELOPMENT.md) · [Third-party terms](THIRD_PARTY.md)

## Shape, space, and export

### Work on the source outline

Create a UFO project or reopen one made by this build. Select a glyph, enter a name and Unicode character, set its advance width, then create and adjust a contour by coordinates or point drag. **Save outline & width** commits numeric changes; pointer drag commits on release. Editing arbitrary external UFOs is deliberately blocked until their unknown data can be preserved.

### Tune a pair

Set explicit kerning for a glyph pair, such as the synthetic **A/V** fixture. In-memory undo and redo help revise the current session; saved UFO data survives reopening. A persistent undo journal is still on the roadmap.

### Export without losing the editable source

**Export TTF** compiles the saved UFO to a separate font file. The bounded export path independently parses its tables and shaped pair advances before replacing a prior output, and a failed compile leaves that prior TTF intact. The UFO stays the editable master; these checks cover specific glyphs and pairs, not general font compatibility.

## Try the Windows preview

Use a Rust toolchain that supports edition 2024. From the repository root:

```powershell
cargo test --locked --features desktop
cargo run --locked --features desktop --bin fontstrike-desktop
```

A fresh checkout needs registry access for uncached dependencies; add `--offline` after the cache is populated. [Development and verification](docs/DEVELOPMENT.md) gives the CLI commands, native controls, test scope, and source safeguards.

## Current status

This is a **Windows engineering preview**, with no verified installer or complete general-purpose font claim. The accepted synthetic fixture compiled to TTF and passed independent table, outline, width, and AV/To kerning checks. External UFO editing, richer point tools, persistent recovery, shaped proof in the app, accessibility, and native pointer acceptance remain open.

No first-party license grant is stated for FontStrike in this checkout. [THIRD_PARTY.md](THIRD_PARTY.md) records identified upstream material and its terms. Explore the other [PWDS Strike repositories](https://github.com/Gerbzz?tab=repositories&q=Strike).
