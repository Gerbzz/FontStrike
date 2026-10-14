<p align="center">
  <img src="assets/branding/FontStrike.png" alt="Original FontStrike woodpecker logo" width="160" height="160">
</p>

<h1 align="center">FontStrike</h1>

<p align="center">
  <a href="https://prowebdevsolutions.com"><picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/branding/PWDS-logo-orange.svg">
    <img src="assets/branding/PWDS-logo.png" alt="PWDS company logo" width="120">
  </picture></a>
</p>

<p align="center"><strong>Draw the shape of a letter, then keep it editable.</strong></p>

FontStrike is a small Windows type-design workspace for outlining glyphs, setting widths and kerning pairs, and exporting a TTF from a UFO source project.

<p align="center"><a href="#try-the-windows-preview">Try the source</a> · <a href="docs/DEVELOPMENT.md">Detailed workflow and verification</a> · <a href="THIRD_PARTY.md">Third-party terms</a> · <a href="assets/branding">Original artwork</a></p>

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

## PWDS brand artwork

The company mark in this masthead is the existing [official PWDS logo](https://github.com/Gerbzz/PWDS-Website/blob/main/public/prowebdevsolutions-logo.png), with the [official orange display asset](https://prowebdevsolutions.com/images/prowebdevsolutions-logo-orange.svg) for dark backgrounds. Any app logo above it remains the original Strike suite artwork.
