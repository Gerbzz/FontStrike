// Independent validation of FontStrike exports with fontkit and opentype.js,
// plus a hand-written sfnt checksum check. Usage: node validate.cjs NODE_DIR FONT.ttf "AV" "To" ...
const path = require('path');
const fs = require('fs');
const [nodeDir, fontPath, ...samples] = process.argv.slice(2);
const req = (m) => require(path.join(nodeDir, 'node_modules', m));
const fontkit = req('fontkit');
const opentype = req('opentype.js');
const buf = fs.readFileSync(fontPath);
let failures = 0;
const check = (ok, msg) => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${msg}`); if (!ok) failures++; };

// 1. sfnt structure and checksums (independent of both libraries).
const numTables = buf.readUInt16BE(4);
const sum = (b, off, len) => { let s = 0; for (let i = 0; i < len; i += 4) { const w = i + 4 <= len ? b.readUInt32BE(off + i) : Buffer.concat([b.subarray(off + i, off + len), Buffer.alloc(4)]).readUInt32BE(0); s = (s + w) >>> 0; } return s; };
const tags = [];
let headOff = -1;
for (let i = 0; i < numTables; i++) {
  const rec = 12 + i * 16;
  const tag = buf.toString('latin1', rec, rec + 4);
  const checksum = buf.readUInt32BE(rec + 4), off = buf.readUInt32BE(rec + 8), len = buf.readUInt32BE(rec + 12);
  tags.push(tag);
  check(off % 4 === 0 && off + len <= buf.length, `table ${tag} offset ${off} length ${len} within file and 4-byte aligned`);
  if (tag === 'head') { headOff = off; const tmp = Buffer.from(buf); tmp.writeUInt32BE(0, off + 8); check(sum(tmp, off, len) === checksum, `table head checksum`); }
  else check(sum(buf, off, len) === checksum, `table ${tag} checksum 0x${checksum.toString(16)}`);
}
console.log(`tables: ${tags.join(' ')}`);
const whole = Buffer.from(buf); const adj = whole.readUInt32BE(headOff + 8); whole.writeUInt32BE(0, headOff + 8);
check(((0xB1B0AFBA - sum(whole, 0, whole.length)) >>> 0) === adj, `head.checkSumAdjustment 0x${adj.toString(16)}`);

// 2. fontkit.
const fk = fontkit.create(buf);
console.log(`fontkit: family="${fk.familyName}" subfamily="${fk.subfamilyName}" upm=${fk.unitsPerEm} glyphs=${fk.numGlyphs} chars=${fk.characterSet.map(c => String.fromCodePoint(c)).join('')}`);
check(fk.numGlyphs > 0 && fk.unitsPerEm === 1000, 'fontkit parsed font with 1000 upm');
// 3. opentype.js.
const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.length);
const ot = opentype.parse(ab);
console.log(`opentype.js: family="${ot.names.fontFamily.en}" glyphs=${ot.numGlyphs} upm=${ot.unitsPerEm}`);
check(ot.numGlyphs === fk.numGlyphs, 'opentype.js and fontkit agree on glyph count');
for (const s of samples) {
  const run = fk.layout(s);
  const fkAdv = run.positions.reduce((a, p) => a + p.xAdvance, 0);
  const plain = [...s].reduce((a, c) => a + fk.glyphForCodePoint(c.codePointAt(0)).advanceWidth, 0);
  const glyphs = ot.stringToGlyphs(s);
  let otAdv = 0; for (let i = 0; i < glyphs.length; i++) { otAdv += glyphs[i].advanceWidth; if (i + 1 < glyphs.length) otAdv += ot.getKerningValue(glyphs[i], glyphs[i + 1]); }
  console.log(`  "${s}": unkerned=${plain} fontkit(GPOS)=${fkAdv} kern=${fkAdv - plain} opentype.js=${otAdv}`);
  check(fkAdv === otAdv, `"${s}" fontkit and opentype.js agree (${fkAdv})`);
}
for (const c of [...new Set(samples.join(''))]) {
  const g = fk.glyphForCodePoint(c.codePointAt(0)); const b = g.bbox;
  console.log(`  glyph ${g.name ?? g.id} "${c}": advance=${g.advanceWidth} bbox=[${b.minX},${b.minY},${b.maxX},${b.maxY}] path commands=${g.path.commands.length}`);
}
console.log(failures === 0 ? 'RESULT: all checks passed' : `RESULT: ${failures} check(s) failed`);
process.exit(failures ? 1 : 0);
