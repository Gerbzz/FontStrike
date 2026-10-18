#!/usr/bin/env python3
"""Independent validation of FontStrike-exported fonts with fontTools and
OpenType Sanitizer. Shares no code with fontc, ttf-parser, or rustybuzz.

Usage: python tools/validate_fonttools.py FONT.ttf "A V -80" "T o -60" ...

Each "LEFT RIGHT VALUE" argument (glyph names) is an expected GPOS pair
adjustment. For every font this:

1. loads it with fontTools.ttLib.TTFont and decompiles every table;
2. round-trips it through TTX XML (saveXML -> importXML -> compile) and
   reloads the compiled result;
3. runs ots-sanitize (OpenType Sanitizer, as used by Chrome and Firefox);
4. reads GPOS pair kerning with fontTools and compares with the expected
   values.

Exits non-zero on any failure.
"""

import io
import subprocess
import sys

from fontTools.ttLib import TTFont


def gpos_pairs(font):
    """Glyph-pair x-advance adjustments from GPOS PairPos lookups."""
    pairs = {}
    if "GPOS" not in font:
        return pairs
    gpos = font["GPOS"].table
    for lookup in gpos.LookupList.Lookup:
        subtables = lookup.SubTable
        if lookup.LookupType == 9:  # extension
            subtables = [s.ExtSubTable for s in subtables]
        for st in subtables:
            if getattr(st, "LookupType", 2) != 2:
                continue
            first = st.Coverage.glyphs
            if st.Format == 1:
                for left, pset in zip(first, st.PairSet):
                    for rec in pset.PairValueRecord:
                        v = getattr(rec.Value1, "XAdvance", 0) if rec.Value1 else 0
                        pairs.setdefault((left, rec.SecondGlyph), v)
            elif st.Format == 2:
                c1 = st.ClassDef1.classDefs
                c2 = st.ClassDef2.classDefs
                seconds = font.getGlyphOrder()
                for left in first:
                    r1 = st.Class1Record[c1.get(left, 0)]
                    for right in seconds:
                        rec = r1.Class2Record[c2.get(right, 0)]
                        v = getattr(rec.Value1, "XAdvance", 0) if rec.Value1 else 0
                        if v:
                            pairs.setdefault((left, right), v)
    return pairs


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    path, expected = argv[1], argv[2:]
    failures = 0

    def check(ok, message):
        nonlocal failures
        print(("PASS  " if ok else "FAIL  ") + message)
        if not ok:
            failures += 1

    font = TTFont(path)
    tags = sorted(font.keys())
    for tag in tags:
        if tag == "GlyphOrder":
            continue
        try:
            table = font[tag]  # forces a full decompile of the table
            if hasattr(table, "ensureDecompiled"):
                table.ensureDecompiled()
            check(True, f"TTFont decompiles table {tag}")
        except Exception as e:  # noqa: BLE001
            check(False, f"table {tag}: {e!r}")
    order = font.getGlyphOrder()
    cmap = font.getBestCmap() or {}
    hmtx = font["hmtx"].metrics
    name = font["name"].getDebugName(1)
    print(f"INFO  family={name!r} upm={font['head'].unitsPerEm} glyphs={len(order)}")
    print(f"INFO  glyph order: {' '.join(order)}")
    print("INFO  cmap: " + " ".join(f"U+{cp:04X}->{g}" for cp, g in sorted(cmap.items())))
    print("INFO  advances: " + " ".join(f"{g}={hmtx[g][0]}" for g in order))

    # TTX round trip.
    xml = io.StringIO()
    font.saveXML(xml)
    again = TTFont()
    again.importXML(io.StringIO(xml.getvalue()))
    buf = io.BytesIO()
    again.save(buf)
    reloaded = TTFont(io.BytesIO(buf.getvalue()))
    check(
        reloaded.getGlyphOrder() == order
        and (reloaded.getBestCmap() or {}) == cmap
        and reloaded["hmtx"].metrics == hmtx,
        f"ttx round trip ({len(xml.getvalue())} chars of XML, {len(buf.getvalue())} bytes) "
        "keeps glyph order, cmap, and advances",
    )

    # OpenType Sanitizer.
    try:
        ots = subprocess.run(
            ["ots-sanitize", path], capture_output=True, text=True, check=False
        )
        out = (ots.stdout + ots.stderr).strip()
        check(ots.returncode == 0, f"ots-sanitize exit {ots.returncode}" + (f": {out}" if out else ""))
    except FileNotFoundError:
        check(False, "ots-sanitize not found (pip install opentype-sanitizer)")

    # GPOS kerning.
    pairs = gpos_pairs(font)
    for spec in expected:
        left, right, value = spec.split()
        got = pairs.get((left, right))
        check(got == int(value), f"GPOS kern {left} {right} = {got} (expected {value})")

    print(f"{'OK' if failures == 0 else 'FAILED'}: {path} ({failures} failure(s))")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
