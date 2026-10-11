# FontStrike source and dependency attribution

- **VectorCraft path donor:** https://github.com/storytold/vectorcraft at commit `231a97dd51a8dc39f62449ee81bc490f7ba9fe5c`, `crates/geom/src/path.rs` copied unchanged to `third_party/vectorcraft/path.rs`. SHA-256 `D14A9847610D18298EE172CB7C010AB04C9A30130B3F473A5D2BEA5B80CAE265`. Its included `third_party/vectorcraft/LICENSE-MIT` preserves the MIT notice. No brand or asset was copied. The small `EPS` host constant in `src/lib.rs` replaces its original crate context; the path file is unmodified.
- **norad 0.18.4:** MIT OR Apache-2.0, https://github.com/linebender/norad. Selected to read/write UFO 3; release 0.18.4 shares Kurbo 0.13 with the pinned VectorCraft donor. The newer 0.19 line has not yet been adopted or compatibility tested.
- **kurbo 0.13.1:** Apache-2.0 OR MIT, https://github.com/linebender/kurbo.
- **serde 1.0.228 and test-only serde_json 1.0.149:** MIT OR Apache-2.0, https://github.com/serde-rs.
- **fontc 1.0.0:** MIT/Apache-2.0, https://github.com/googlefonts/fontc. Pinned Rust library API compiles local UFO sources to TrueType bytes. The project source is not replaced by compiled output.
- **ttf-parser 0.25.1:** MIT OR Apache-2.0, https://github.com/harfbuzz/ttf-parser. Independently parses exported tables, mappings, metrics, and outline bounds.
- **test-only rustybuzz 0.20.1:** MIT, https://github.com/harfbuzz/rustybuzz. Shapes AV and To acceptance pairs independently of the compiler.
- **sha2 0.10.9:** MIT OR Apache-2.0, https://github.com/RustCrypto/hashes. Hashes the complete UFO file tree so changes outside the app are refused before replacing source.

`Cargo.lock` records the tested transitive graph. The pinned compiler and parser crates were checked against their upstream repository and cached license metadata. The separate combined Windows normal/build-dependency notice audit is not included in this standalone source checkout. A binary distribution must include a verified notice bundle for its exact contents. This checkout contains no binaries. DesignCraft font shaping remains a reviewed donor candidate, with no source copied into this increment.

The native Windows preview adds registry crates `eframe 0.30.0` (MIT OR Apache-2.0, https://github.com/emilk/egui), `rfd 0.15.4` (MIT, https://github.com/PolyMeilex/rfd), and `png 0.18.1` (MIT OR Apache-2.0, https://github.com/image-rs/image-png) for the QA capture. The visual canvas uses the above copied VectorCraft `PathData`, `SubPath`, and `Anchor` methods; no VectorCraft UI source has been copied.

