# FontStrike development and verification

Start with the [overview](../README.md). This page covers architecture, commands, tests,
and how screenshots and validation records are produced.

## Architecture

| Module | Responsibility |
| --- | --- |
| `src/project.rs` | `FontProject`: the norad `Font` model, every edit, part-based undo/redo (only the glyphs, kerning, groups, info, or lib an edit touches are snapshotted), gestures that group a drag into one step, dirty tracking. |
| `src/outline.rs` | Point-level contour operations on UFO points (segments, insert, delete/merge, line↔curve, reverse, ellipse/polygon builders, bounds with resolved components, hit testing). |
| `src/storage.rs` | Snapshot of the UFO tree, classification of model-owned vs. verbatim files, the stage → verify → journal → swap save, `recover`, autosave. |
| `src/export.rs` | `ExportJob`: write the model to a temporary UFO, compile with fontc, verify with ttf-parser and rustybuzz, atomic replace. |
| `src/session.rs` | UI-independent document controller: New/Open/Save As/Export flows, cancelled dialogs, unsaved-change guards, recovery and autosave prompts. |
| `src/raster.rs` | Small anti-aliased non-zero rasteriser for thumbnails and previews. |
| `src/demo.rs` | The synthetic test font. |
| `src/bin/desktop.rs` | egui interface only. |
| `src/main.rs` | `fontstrike` CLI. |

### Save pipeline

1. Refuse if an interrupted save is pending or the UFO changed on disk since it was opened.
2. Write the model to `<name>.ufo.fontstrike-staging` (always UFO 3).
3. Copy verbatim files (unknown root files and folders, unknown files in layer folders) and
   merge unknown `fontinfo.plist` / `layerinfo.plist` keys.
4. Reload the stage with norad and compare font info, lib, groups, kerning, features, and
   every glyph of every layer with the model. Check that every original file exists in the
   stage unless the model owns it (glyph files, known plists, `data/`, `images/`).
5. Flush files, write `<name>.ufo.fontstrike-journal`, rename the UFO to
   `<name>.ufo.fontstrike-backup`, rename the stage into place, remove backup and journal.

`recover` rolls a journalled save forward, restores a backup when the UFO is missing, and
renames any other leftover to `<name>.ufo.fontstrike-recovered-N` rather than deleting it.

## Commands

```powershell
cargo fmt --check
cargo clippy --locked --features desktop --all-targets
cargo test --locked --features desktop
cargo run --locked --features desktop --bin fontstrike-desktop
```

Desktop flags (used for QA captures):

```text
--demo PATH.ufo          open, or create, the synthetic test font
--project PATH.ufo       open a UFO
--select GLYPH           select a glyph
--point C:P              select contour C, point P
--about                  open the About dialog
--size WxH               window size (minimum 900x600)
--capture FILE.png       save the rendered window after ~20 frames and exit
```

CLI:

```text
fontstrike new FONT.ufo FAMILY STYLE
fontstrike demo FONT.ufo
fontstrike rect FONT.ufo GLYPH UNICODE|- WIDTH X_MIN Y_MIN X_MAX Y_MAX
fontstrike width FONT.ufo GLYPH ADVANCE
fontstrike kern FONT.ufo LEFT RIGHT AMOUNT
fontstrike export FONT.ufo OUTPUT.ttf
fontstrike status FONT.ufo
fontstrike recover FONT.ufo
```

## Test map

| File | Covers |
| --- | --- |
| `tests/editing.rs` | Undo and redo of each of 20 edit types, save and reopen after each; gestures and cancelled gestures; point semantics; side bearings; refused invalid edits. |
| `tests/preservation.rs` | A hand-written rich UFO (extra layer, features, groups, data, unknown files and keys, components, anchors, guidelines, identifiers, qcurves) edited and saved with everything else intact; byte-identical re-save; group kerning; rename across layers; UFO 2 upgrade; external-change refusal. |
| `tests/corrupt.rs` | Missing or garbage metainfo, format 4, malformed and unknown-element glif, missing glif, truncated plist, wrong value types, `.ufoz`, missing folder; all refused without changing a byte. Failed open keeps the current document. |
| `tests/recovery.rs` | Interruption after stage, journal, backup rename, and swap, each recovered to the old or new version; legacy backup restore; session recovery and autosave restore/discard; repeated saves and exports leave no temporary files; failed export keeps the previous file; export does not modify the source. |
| `tests/session.rs` | Cancelled New/Open/Save As/Export dialogs write nothing; unsaved-change guard; Save As copies unmodelled files. |
| `tests/export.rs` | Readback of tables, advances, outline bounds; rustybuzz-shaped kerning for the synthetic font and for group kerning in the rich fixture. |
| `tests/source.rs`, `tests/cli.rs` | VectorCraft path round trip, CLI workflow. |
| unit tests | Outline operations, rasteriser, VectorCraft path donor. |

On the development machine, Windows Application Control once blocked the (empty) unit-test
executable generated for the desktop binary (`os error 4551`) while Smart App Control was still
enforcing; later runs of the same command passed. If it happens, run
`cargo test --locked --features desktop --no-fail-fast` to see every other result.

## Screenshots and validation

Screenshots in `docs/screenshots/` are produced with `--capture`, which saves the frame
egui rendered for the real window. Export validation commands and recorded output are in
[VALIDATION.md](VALIDATION.md); `tools/validate-export.cjs` is the Node.js script used.

### Linux CI native-window capture

The opt-in `desktop-x11` feature adds eframe's official X11 platform support to the
existing `desktop` feature. It keeps the same glow/OpenGL renderer, native event loop and
`--capture` screenshot event; there is no offscreen/headless UI substitute. The default
Windows feature, app source, product logo, and publisher assets are unchanged.

The Linux CI job runs all source/desktop tests, builds with `--locked --features desktop-x11`,
and launches the actual editor under Xvfb with Mesa llvmpipe. `glxinfo -B`, per-view logs
and exact source/image hashes accompany the five native PNGs. The validator rejects
missing, duplicate, stale, blank or undersized frames. Pillow only decodes/verifies the
app-produced PNGs; it never generates or edits captured pixels. Its temporary unit-test
fixtures are not application screenshots and are never uploaded.

The hosted Windows capture attempt at source `f9af889962cf53c992bdb861bc776f8681898dfe`
(run 40020031567) failed because that runner could not provide OpenGL 2.0. Its font
source tests, branding tests and native build passed, but it produced no PNG. The
Windows job and strict failure remain intact; Linux evidence must never be labelled
Windows GUI acceptance. This CI route is not a general Linux packaging/support claim.

## Source terms

No first-party licence grant is stated for this app in this checkout. `THIRD_PARTY.md`
records attribution for upstream material.
