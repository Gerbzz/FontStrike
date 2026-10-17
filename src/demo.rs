//! A clearly labelled synthetic test font, used for screenshots, validation,
//! and manual QA. Every outline here is drawn from simple coordinates in this
//! file; no third-party font data is involved.

use crate::outline::{ellipse, polygon};
use crate::{FontProject, Result};
use std::path::Path;

pub const FAMILY: &str = "FontStrike Test Sans";
pub const STYLE: &str = "Synthetic";

/// (glyph name, character, advance width, closed polygon contours)
type Spec<'a> = (&'a str, char, f64, &'a [&'a [(f64, f64)]]);

/// Creates the synthetic test font at `root` (which must not exist).
pub fn build(root: &Path) -> Result<FontProject> {
    let mut p = FontProject::create(root, FAMILY, STYLE)?;
    let glyphs: &[Spec] = &[
        (
            "A",
            'A',
            640.0,
            &[
                &[
                    (30.0, 0.0),
                    (140.0, 0.0),
                    (190.0, 160.0),
                    (450.0, 160.0),
                    (500.0, 0.0),
                    (610.0, 0.0),
                    (375.0, 700.0),
                    (265.0, 700.0),
                ],
                &[(225.0, 270.0), (320.0, 570.0), (415.0, 270.0)],
            ],
        ),
        (
            "V",
            'V',
            620.0,
            &[&[
                (255.0, 0.0),
                (365.0, 0.0),
                (600.0, 700.0),
                (490.0, 700.0),
                (310.0, 150.0),
                (130.0, 700.0),
                (20.0, 700.0),
            ]],
        ),
        (
            "T",
            'T',
            580.0,
            &[&[
                (235.0, 0.0),
                (345.0, 0.0),
                (345.0, 600.0),
                (550.0, 600.0),
                (550.0, 700.0),
                (30.0, 700.0),
                (30.0, 600.0),
                (235.0, 600.0),
            ]],
        ),
        (
            "H",
            'H',
            660.0,
            &[&[
                (70.0, 0.0),
                (180.0, 0.0),
                (180.0, 300.0),
                (480.0, 300.0),
                (480.0, 0.0),
                (590.0, 0.0),
                (590.0, 700.0),
                (480.0, 700.0),
                (480.0, 400.0),
                (180.0, 400.0),
                (180.0, 700.0),
                (70.0, 700.0),
            ]],
        ),
        (
            "n",
            'n',
            560.0,
            &[&[
                (70.0, 0.0),
                (170.0, 0.0),
                (170.0, 380.0),
                (390.0, 380.0),
                (390.0, 0.0),
                (490.0, 0.0),
                (490.0, 480.0),
                (70.0, 480.0),
            ]],
        ),
    ];
    for (name, c, width, contours) in glyphs {
        p.add_glyph(name, &[*c], *width)?;
        for contour in contours.iter() {
            p.add_contour(name, polygon(contour)?)?;
        }
    }
    p.add_glyph("o", &['o'], 560.0)?;
    p.add_contour("o", ellipse(40.0, -10.0, 520.0, 490.0)?)?;
    let mut counter = ellipse(140.0, 80.0, 420.0, 400.0)?;
    crate::outline::reverse_contour(&mut counter)?;
    p.add_contour("o", counter)?;
    p.set_kerning("A", "V", -80.0)?;
    p.set_kerning("T", "o", -60.0)?;
    p.set_kerning("V", "A", -70.0)?;
    p.save()?;
    Ok(p)
}
