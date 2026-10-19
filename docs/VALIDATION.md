# Export validation record

FontStrike checks every export itself (ttf-parser readback of tables,
mappings, advances, and outline bounds, plus rustybuzz shaping of kerning
pairs). This page records an **independent** check of exported fonts with
tools that share no code with fontc, ttf-parser, or rustybuzz.

## Validators

| Tool | Version | Licence | Used for |
| --- | --- | --- | --- |
| [fontkit](https://github.com/foliojs/fontkit) (Node.js) | 2.0.4 | MIT | parse, GPOS layout of test strings, glyph bounds |
| [opentype.js](https://github.com/opentypejs/opentype.js) (Node.js) | 1.3.4 | MIT | parse, GPOS pair kerning |
| `tools/validate-export.cjs` | this repo | — | runs both, plus a hand-written sfnt table/whole-file checksum check |
| [fontTools](https://github.com/fonttools/fonttools) (Python) | 4.67.0 | MIT | `TTFont` load and full table decompile, `ttx` XML round trip, GPOS pair kerning |
| [OpenType Sanitizer](https://github.com/khaledhosny/ots) via `opentype-sanitizer` (Python wheel) | 9.2.0 | BSD-3-Clause | `ots-sanitize` (`python -m ots`), the sanitizer browsers run on web fonts |
| `tools/validate_fonttools.py` | this repo | — | runs the fontTools and OTS checks and compares kerning with expected values |

The validators were installed with `npm install fontkit@2.0.4 opentype.js@1.3.4`
into a scratch folder outside the repository (Node.js v24.18.0). They are
not dependencies of FontStrike and nothing from them ships with it.

fontkit and opentype.js were run by hand on the Windows development machine (2026-10-17).
fontTools and OTS could not run there (no Python), so they run in CI instead: the
`ubuntu-latest` job of `.github/workflows/ui-review.yml` installs them with pip (Python
3.12) and validates the fonts that `cargo test` exported on that runner. Output below is
from [CI run 39197467448](https://github.com/Gerbzz/FontStrike/actions/runs/39197467448) (commit 696e830, 2026-10-19).

## Fonts checked

1. `FontStrikeTestSans.ttf`, the synthetic test font
   (`cargo run --bin fontstrike -- demo …` then `… export …`): 8 glyphs,
   counter shapes in A and o, cubic curves in o, three kerning pairs.
2. `RichFixture.ttf`, compiled from the hand-written UFO in
   `tests/common/mod.rs` (`rich_ufo`): components (Aacute = A + acutecomb),
   quadratic `qcurve` contours, a background layer, features.fea, and group
   kerning (`public.kern1.A` × `public.kern2.o` = -40) with a glyph exception
   (A × o = -55).

Expected kerning: AV -80, VA -70, To -60 (synthetic); Ao -55 (exception),
Áo -40 (group) (rich fixture).

## Commands

```text
cargo run --locked --bin fontstrike -- demo FontStrikeTestSans.ufo
cargo run --locked --bin fontstrike -- export FontStrikeTestSans.ufo FontStrikeTestSans.ttf
node tools/validate-export.cjs <node-dir-with-fontkit> FontStrikeTestSans.ttf AV VA To HH AVATAR
node tools/validate-export.cjs <node-dir-with-fontkit> RichFixture.ttf Ao Áo oA
```

FontStrike's own export report for the two fonts (recorded 2026-10-17):

```text
Compiled and verified FontStrikeTestSans.ttf (1456 bytes)
  8 glyphs, 7 mappings and advances, 6 outline bounds, 3 shaped kerning pairs verified
Compiled and verified RichFixture.ttf (1264 bytes)
  6 glyphs, 5 mappings and advances, 4 outline bounds, 3 shaped kerning pairs verified
```

## Recorded output: FontStrikeTestSans.ttf (fontkit, opentype.js, checksums)

```text
PASS  table GPOS offset 1348 length 106 within file and 4-byte aligned
PASS  table GPOS checksum 0x2b522452
PASS  table OS/2 offset 312 length 96 within file and 4-byte aligned
PASS  table OS/2 checksum 0x5f115b8e
PASS  table cmap offset 440 length 92 within file and 4-byte aligned
PASS  table cmap checksum 0x1710106
PASS  table glyf offset 552 length 324 within file and 4-byte aligned
PASS  table glyf checksum 0xd74adc7
PASS  table head offset 188 length 54 within file and 4-byte aligned
PASS  table head checksum
PASS  table hhea offset 244 length 36 within file and 4-byte aligned
PASS  table hhea checksum 0x66005d0
PASS  table hmtx offset 408 length 32 within file and 4-byte aligned
PASS  table hmtx checksum 0x11760140
PASS  table loca offset 532 length 18 within file and 4-byte aligned
PASS  table loca checksum 0x19d0161
PASS  table maxp offset 280 length 32 within file and 4-byte aligned
PASS  table maxp checksum 0xb0019
PASS  table name offset 876 length 418 within file and 4-byte aligned
PASS  table name checksum 0x215d3ed5
PASS  table post offset 1296 length 50 within file and 4-byte aligned
PASS  table post checksum 0x6e00e8
tables: GPOS OS/2 cmap glyf head hhea hmtx loca maxp name post
PASS  head.checkSumAdjustment 0x56238028
fontkit: family="FontStrike Test Sans Synthetic" subfamily="Regular" upm=1000 glyphs=8 chars= AHTVno￿
PASS  fontkit parsed font with 1000 upm
opentype.js: family="FontStrike Test Sans Synthetic" glyphs=8 upm=1000
PASS  opentype.js and fontkit agree on glyph count
  "AV": unkerned=1260 fontkit(GPOS)=1180 kern=-80 opentype.js=1180
PASS  "AV" fontkit and opentype.js agree (1180)
  "VA": unkerned=1260 fontkit(GPOS)=1190 kern=-70 opentype.js=1190
PASS  "VA" fontkit and opentype.js agree (1190)
  "To": unkerned=1140 fontkit(GPOS)=1080 kern=-60 opentype.js=1080
PASS  "To" fontkit and opentype.js agree (1080)
  "HH": unkerned=1320 fontkit(GPOS)=1320 kern=0 opentype.js=1320
PASS  "HH" fontkit and opentype.js agree (1320)
  "AVATAR": unkerned=3720 fontkit(GPOS)=3570 kern=-150 opentype.js=3570
PASS  "AVATAR" fontkit and opentype.js agree (3570)
  glyph A "A": advance=640 bbox=[30,0,610,700] path commands=13
  glyph V "V": advance=620 bbox=[20,0,600,700] path commands=8
  glyph T "T": advance=580 bbox=[30,0,550,700] path commands=9
  glyph o "o": advance=560 bbox=[40,-10,520,490] path commands=30
  glyph H "H": advance=660 bbox=[70,0,590,700] path commands=13
  glyph .notdef "R": advance=600 bbox=[60,0,540,700] path commands=10
RESULT: all checks passed
```

## Recorded output: RichFixture.ttf (fontkit, opentype.js, checksums)

```text
PASS  table GDEF offset 1036 length 24 within file and 4-byte aligned
PASS  table GDEF checksum 0x110006
PASS  table GPOS offset 1060 length 204 within file and 4-byte aligned
PASS  table GPOS checksum 0x9b799c92
PASS  table OS/2 offset 328 length 96 within file and 4-byte aligned
PASS  table OS/2 checksum 0x5f535d7b
PASS  table cmap offset 448 length 84 within file and 4-byte aligned
PASS  table cmap checksum 0x1210411
PASS  table glyf offset 548 length 126 within file and 4-byte aligned
PASS  table glyf checksum 0x3bbf1fa8
PASS  table head offset 204 length 54 within file and 4-byte aligned
PASS  table head checksum
PASS  table hhea offset 260 length 36 within file and 4-byte aligned
PASS  table hhea checksum 0x5d50510
PASS  table hmtx offset 424 length 24 within file and 4-byte aligned
PASS  table hmtx checksum 0x9a7005a
PASS  table loca offset 532 length 14 within file and 4-byte aligned
PASS  table loca checksum 0x720059
PASS  table maxp offset 296 length 32 within file and 4-byte aligned
PASS  table maxp checksum 0xc000c
PASS  table name offset 676 length 304 within file and 4-byte aligned
PASS  table name checksum 0x15e02d3f
PASS  table post offset 980 length 56 within file and 4-byte aligned
PASS  table post checksum 0xc7bfec80
tables: GDEF GPOS OS/2 cmap glyf head hhea hmtx loca maxp name post
PASS  head.checkSumAdjustment 0x6e08ac2a
fontkit: family="Rich Fixture" subfamily="Regular" upm=1000 glyphs=6 chars= AoÁ́￿
PASS  fontkit parsed font with 1000 upm
opentype.js: family="Rich Fixture" glyphs=6 upm=1000
PASS  opentype.js and fontkit agree on glyph count
  "Ao": unkerned=1120 fontkit(GPOS)=1065 kern=-55 opentype.js=1065
PASS  "Ao" fontkit and opentype.js agree (1065)
  "Áo": unkerned=1120 fontkit(GPOS)=1080 kern=-40 opentype.js=1080
PASS  "Áo" fontkit and opentype.js agree (1080)
  "oA": unkerned=1120 fontkit(GPOS)=1120 kern=0 opentype.js=1120
PASS  "oA" fontkit and opentype.js agree (1120)
  glyph A "A": advance=600 bbox=[20,0,580,700] path commands=4
  glyph o "o": advance=520 bbox=[40,0,480,500] path commands=7
  glyph Aacute "Á": advance=600 bbox=[20,0,580,840] path commands=8
RESULT: all checks passed
```

Both runs exited with status 0. (fontkit's character set listing shows
U+FFFF, the cmap format 4 terminator segment; it is not a mapped glyph.)

## Recorded output: fontTools and OTS (CI, Linux)

From [CI run 39197467448](https://github.com/Gerbzz/FontStrike/actions/runs/39197467448), job `ubuntu-latest`, step "Validate exported fonts with
fontTools, ttx, and OpenType Sanitizer". The step first round-trips each font through `ttx`
(`ttx -o F.ttx F.ttf`, then `ttx -o F.roundtrip.ttf F.ttx`) and sanitizes the rebuilt font,
then runs `tools/validate_fonttools.py` on the original export:

```text
File sanitized successfully!
PASS  ots-sanitize (python -m ots) on the ttx round-trip of FontStrikeTestSans.ttf
File sanitized successfully!
PASS  ots-sanitize (python -m ots) on the ttx round-trip of RichFixture.ttf
PASS  TTFont decompiles table GPOS
PASS  TTFont decompiles table OS/2
PASS  TTFont decompiles table cmap
PASS  TTFont decompiles table glyf
PASS  TTFont decompiles table head
PASS  TTFont decompiles table hhea
PASS  TTFont decompiles table hmtx
PASS  TTFont decompiles table loca
PASS  TTFont decompiles table maxp
PASS  TTFont decompiles table name
PASS  TTFont decompiles table post
INFO  family='FontStrike Test Sans Synthetic' upm=1000 glyphs=8
INFO  glyph order: .notdef A H T V n o space
INFO  cmap: U+0020->space U+0041->A U+0048->H U+0054->T U+0056->V U+006E->n U+006F->o
INFO  advances: .notdef=600 A=640 H=660 T=580 V=620 n=560 o=560 space=250
PASS  ttx round trip (13932 chars of XML, 1456 bytes) keeps glyph order, cmap, and advances
PASS  ots-sanitize (python -m ots) exit 0: File sanitized successfully!
PASS  GPOS kern A V = -80 (expected -80)
PASS  GPOS kern V A = -70 (expected -70)
PASS  GPOS kern T o = -60 (expected -60)
OK: FontStrikeTestSans.ttf (0 failure(s))
PASS  TTFont decompiles table GDEF
PASS  TTFont decompiles table GPOS
PASS  TTFont decompiles table OS/2
PASS  TTFont decompiles table cmap
PASS  TTFont decompiles table glyf
PASS  TTFont decompiles table head
PASS  TTFont decompiles table hhea
PASS  TTFont decompiles table hmtx
PASS  TTFont decompiles table loca
PASS  TTFont decompiles table maxp
PASS  TTFont decompiles table name
PASS  TTFont decompiles table post
INFO  family='Rich Fixture' upm=1000 glyphs=6
INFO  glyph order: .notdef space A Aacute acutecomb o
INFO  cmap: U+0020->space U+0041->A U+006F->o U+00C1->Aacute U+0301->acutecomb
INFO  advances: .notdef=500 space=250 A=600 Aacute=600 acutecomb=0 o=520
PASS  ttx round trip (12385 chars of XML, 1264 bytes) keeps glyph order, cmap, and advances
PASS  ots-sanitize (python -m ots) exit 0: File sanitized successfully!
PASS  GPOS kern A o = -55 (expected -55)
PASS  GPOS kern Aacute o = -40 (expected -40)
OK: RichFixture.ttf (0 failure(s))
```

Both fonts pass every check: all tables decompile in fontTools, the TTX round trip keeps
glyph order, cmap, and advances, OTS accepts both the original and the round-tripped font,
and the GPOS kerning fontTools reads matches the source (AV -80, VA -70, To -60; Ao -55 from
the glyph exception, Áo -40 from group kerning). The fonts and these logs are uploaded as
the `test-fonts-Linux` artifact of that run.

To reproduce elsewhere:

```text
pip install fonttools opentype-sanitizer
FONTSTRIKE_EXPORT_DIR=out/fonts cargo test --locked --features desktop --test export
ttx -o out/fonts/RichFixture.ttx out/fonts/RichFixture.ttf
python -m ots out/fonts/RichFixture.ttf
python tools/validate_fonttools.py out/fonts/RichFixture.ttf "A o -55" "Aacute o -40"
```
