# FontStrike

**by PWDS**

**Draw the shape of a letter.**

FontStrike gives a small type project an editable home. Create glyph contours in a UFO source, adjust widths and kerning pairs, save the source, and compile a TTF for a bounded review.

### Highlights

- Draw and adjust glyph outlines.
- Set advance widths and pair kerning.
- Keep editable UFO source and export a TTF.

## Current status

Windows engineering preview. Export has passed specific glyph and kerning checks, but a completed general-purpose font is not claimed. Editing arbitrary external UFOs, richer point tools, persistent recovery, accessibility, and broad font verification remain open.

## Draw and export a glyph

1. Use **New UFO** or open a UFO created by this build, then select or name a glyph.
2. Create a contour and adjust its points, advance width, and a pair-kerning value.
3. Save the editable outline, then choose **Export TTF**. The generated font is independently parsed before it replaces a prior export.

External UFOs cannot yet be modified safely, and this bounded export is not a general font-compatibility claim.

## Build and use

Launch on Windows from this directory:

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_TARGET_DIR='..\target-fontstrike'
cargo test --locked --features desktop
cargo run --locked --features desktop --bin fontstrike-desktop
```

A fresh checkout needs registry access for its first dependency download. After the registry cache is populated, add `--offline` if an offline build is needed.

Use **New UFO** or **Open UFO**, select a glyph, enter a name and one Unicode character, set its width, create a rectangle contour, then drag or numerically adjust its points. **Save outline & width** commits numeric edits; pointer drag commits on release. **Save pair** writes explicit glyph kerning. **Export TTF** selects a local output path and compiles the saved UFO. Undo/redo is currently in memory for the open session; saved UFO data persists across reopen. Ctrl+S saves the current outline. The `--demo PATH.ufo --capture FILE.png` flags create a source fixture and capture the running native UI for QA.

Command line source workflow:

```text
cargo run --locked --bin fontstrike -- new Example.ufo Example Regular
cargo run --locked --bin fontstrike -- rect Example.ufo A A 620 100 0 520 700
cargo run --locked --bin fontstrike -- rect Example.ufo V V 600 100 0 500 700
cargo run --locked --bin fontstrike -- kern Example.ufo A V -80
cargo run --locked --bin fontstrike -- export Example.ufo Example.ttf
cargo run --locked --bin fontstrike -- status Example.ufo
```

`rect` is a temporary numeric shape-entry command and accepts conventional y-up font coordinates. The library supports arbitrary closed cubic `PathData` outlines and in-memory undo/redo. Opening external UFOs for modification is deliberately blocked until unknown layer, feature, data, and lib preservation fixtures pass; a `.fontstrike-v1` marker identifies files created by this build.

The export acceptance fixture checks `cmap`, `glyf`, `hmtx`, `GPOS`, and `name` tables, independent outline bounds, mapped advance widths, and shaped AV/To pair advances through rustybuzz. It proves those specific glyphs and kerning pairs, not arbitrary font behavior. The editable UFO is the source of truth; a failed compile/readback leaves a previous TTF intact.

Current limits: no persistent undo journal, shaped proof in the app, DesignCraft integration, component editing, point insertion/deletion, or external UFO write support. A whole-tree content snapshot refuses UFO changes made after open; staging refuses to replace a source containing files the writer would omit. TTF compilation currently runs on the UI thread and may pause the window for large UFOs. A completed general-purpose font is not claimed. The next gates are persistent recovery, richer point tools, screen-reader and pointer verification, and an independent real-world font fixture. Planning history and QA captures are not included in this standalone source checkout.

## Source terms

No first-party license grant is stated for this app in this checkout. `THIRD_PARTY.md` records attribution for identified upstream material; the included donor license files, where present, apply to that material.
