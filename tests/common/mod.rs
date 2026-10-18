#![allow(dead_code)]
//! Shared fixtures. Every font written here is synthetic test data.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// A fresh, empty scratch directory unique to this test run.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("fontstrike-tests").join(format!(
        "{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every file under `root` with its bytes (for byte-identical comparisons).
pub fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(d) = pending.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                pending.push(p.clone());
                out.push((
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    b"<dir>".to_vec(),
                ));
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&p).unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}

/// Names of entries directly inside `dir`.
pub fn listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const PLIST_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
"#;

pub fn plist(body: &str) -> String {
    format!("{PLIST_HEAD}{body}\n</plist>\n")
}

/// A hand-written UFO 3 that exercises data FontStrike must not lose:
/// unknown lib keys, a background layer, features, kerning groups, a data
/// folder, unknown root and layer files, unknown fontinfo/layerinfo keys,
/// components, anchors, guidelines, identifiers, a glyph lib, a note, and
/// quadratic (qcurve) contours.
pub fn rich_ufo(root: &Path) {
    write(
        &root.join("metainfo.plist"),
        &plist(
            "<dict><key>creator</key><string>com.example.handwritten</string><key>formatVersion</key><integer>3</integer></dict>",
        ),
    );
    write(
        &root.join("fontinfo.plist"),
        &plist(
            "<dict><key>familyName</key><string>Rich Fixture</string><key>styleName</key><string>Regular</string><key>unitsPerEm</key><integer>1000</integer><key>ascender</key><integer>750</integer><key>descender</key><integer>-250</integer><key>xHeight</key><integer>500</integer><key>capHeight</key><integer>700</integer><key>com.example.futureFontinfoKey</key><string>keep me</string></dict>",
        ),
    );
    write(
        &root.join("lib.plist"),
        &plist(
            "<dict><key>com.example.unknownLibKey</key><dict><key>nested</key><array><integer>1</integer><string>two</string></array></dict><key>public.glyphOrder</key><array><string>.notdef</string><string>space</string><string>A</string><string>Aacute</string><string>acutecomb</string><string>o</string></array></dict>",
        ),
    );
    write(
        &root.join("groups.plist"),
        &plist(
            "<dict><key>public.kern1.A</key><array><string>A</string><string>Aacute</string></array><key>public.kern2.o</key><array><string>o</string></array><key>com.example.notKerning</key><array><string>A</string></array></dict>",
        ),
    );
    write(
        &root.join("kerning.plist"),
        &plist(
            "<dict><key>public.kern1.A</key><dict><key>public.kern2.o</key><integer>-40</integer></dict><key>A</key><dict><key>o</key><integer>-55</integer></dict></dict>",
        ),
    );
    write(
        &root.join("features.fea"),
        "# Hand-written feature file\nlanguagesystem DFLT dflt;\n",
    );
    write(
        &root.join("layercontents.plist"),
        &plist(
            "<array><array><string>public.default</string><string>glyphs</string></array><array><string>public.background</string><string>glyphs.public.background</string></array></array>",
        ),
    );
    write(
        &root.join("glyphs/contents.plist"),
        &plist(
            "<dict><key>.notdef</key><string>_notdef.glif</string><key>space</key><string>space.glif</string><key>A</key><string>A_.glif</string><key>Aacute</key><string>A_acute.glif</string><key>acutecomb</key><string>acutecomb.glif</string><key>o</key><string>o.glif</string></dict>",
        ),
    );
    write(
        &root.join("glyphs/layerinfo.plist"),
        &plist(
            "<dict><key>color</key><string>1,0.75,0,0.7</string><key>com.example.layerKey</key><string>layer data</string></dict>",
        ),
    );
    write(
        &root.join("glyphs/_notdef.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\".notdef\" format=\"2\"><advance width=\"500\"/><outline><contour><point x=\"50\" y=\"0\" type=\"line\"/><point x=\"450\" y=\"0\" type=\"line\"/><point x=\"450\" y=\"700\" type=\"line\"/><point x=\"50\" y=\"700\" type=\"line\"/></contour></outline></glyph>\n",
    );
    write(
        &root.join("glyphs/space.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\"space\" format=\"2\"><advance width=\"250\"/><unicode hex=\"0020\"/></glyph>\n",
    );
    write(
        &root.join("glyphs/A_.glif"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<glyph name="A" format="2">
  <advance width="600"/>
  <unicode hex="0041"/>
  <note>hand-written note</note>
  <guideline x="300" name="center" identifier="guide1"/>
  <anchor x="300" y="700" name="top" identifier="anchor1"/>
  <outline>
    <contour identifier="contour1">
      <point x="20" y="0" type="line" name="bottomLeft" identifier="pt1"/>
      <point x="580" y="0" type="line"/>
      <point x="300" y="700" type="line"/>
    </contour>
  </outline>
  <lib><dict><key>com.example.glyphLib</key><string>glyph data</string></dict></lib>
</glyph>
"#,
    );
    write(
        &root.join("glyphs/A_acute.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\"Aacute\" format=\"2\"><advance width=\"600\"/><unicode hex=\"00C1\"/><outline><component base=\"A\"/><component base=\"acutecomb\" xOffset=\"300\" yOffset=\"0\"/></outline></glyph>\n",
    );
    write(
        &root.join("glyphs/acutecomb.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\"acutecomb\" format=\"2\"><advance width=\"0\"/><unicode hex=\"0301\"/><anchor x=\"0\" y=\"700\" name=\"_top\"/><outline><contour><point x=\"-40\" y=\"740\" type=\"line\"/><point x=\"60\" y=\"740\" type=\"line\"/><point x=\"20\" y=\"840\" type=\"line\"/></contour></outline></glyph>\n",
    );
    write(
        &root.join("glyphs/o.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\"o\" format=\"2\"><advance width=\"520\"/><unicode hex=\"006F\"/><outline><contour><point x=\"260\" y=\"0\" type=\"qcurve\" smooth=\"yes\"/><point x=\"480\" y=\"0\"/><point x=\"480\" y=\"500\"/><point x=\"260\" y=\"500\" type=\"qcurve\" smooth=\"yes\"/><point x=\"40\" y=\"500\"/><point x=\"40\" y=\"0\"/></contour></outline></glyph>\n",
    );
    write(
        &root.join("glyphs/README-unknown.txt"),
        "unknown file inside the glyph folder\n",
    );
    write(
        &root.join("glyphs.public.background/contents.plist"),
        &plist("<dict><key>A</key><string>A_.glif</string></dict>"),
    );
    write(
        &root.join("glyphs.public.background/A_.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\"A\" format=\"2\"><advance width=\"600\"/><outline><contour><point x=\"0\" y=\"0\" type=\"line\"/><point x=\"600\" y=\"0\" type=\"line\"/><point x=\"300\" y=\"650\" type=\"line\"/></contour></outline></glyph>\n",
    );
    write(
        &root.join("data/com.example.tool/settings.json"),
        "{\"keep\": true}\n",
    );
    write(
        &root.join("com.example.unknown-root-file.txt"),
        "unknown root file\n",
    );
    write(
        &root.join("com.example.unknown-dir/inner.bin"),
        "binary-ish payload\n",
    );
}

/// A minimal hand-written UFO 2 source.
pub fn ufo2(root: &Path) {
    write(
        &root.join("metainfo.plist"),
        &plist(
            "<dict><key>creator</key><string>com.example.old</string><key>formatVersion</key><integer>2</integer></dict>",
        ),
    );
    write(
        &root.join("fontinfo.plist"),
        &plist(
            "<dict><key>familyName</key><string>Old Fixture</string><key>styleName</key><string>Regular</string><key>unitsPerEm</key><integer>1000</integer></dict>",
        ),
    );
    write(
        &root.join("groups.plist"),
        &plist("<dict><key>@MMK_L_A</key><array><string>A</string></array></dict>"),
    );
    write(
        &root.join("kerning.plist"),
        &plist("<dict><key>@MMK_L_A</key><dict><key>A</key><integer>-20</integer></dict></dict>"),
    );
    write(
        &root.join("glyphs/contents.plist"),
        &plist(
            "<dict><key>.notdef</key><string>_notdef.glif</string><key>A</key><string>A_.glif</string></dict>",
        ),
    );
    write(
        &root.join("glyphs/_notdef.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\".notdef\" format=\"1\"><advance width=\"500\"/></glyph>\n",
    );
    write(
        &root.join("glyphs/A_.glif"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<glyph name=\"A\" format=\"1\"><advance width=\"600\"/><unicode hex=\"0041\"/><outline><contour><point x=\"20\" y=\"0\" type=\"line\"/><point x=\"580\" y=\"0\" type=\"line\"/><point x=\"300\" y=\"700\" type=\"line\"/></contour></outline></glyph>\n",
    );
}
