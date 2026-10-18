use fontstrike::FontProject;
use std::path::Path;

fn usage() -> &'static str {
    "FontStrike commands:
  new FONT.ufo FAMILY STYLE              create a UFO 3 source
  demo FONT.ufo                          create the synthetic test font
  rect FONT.ufo GLYPH UNICODE|- WIDTH X_MIN Y_MIN X_MAX Y_MAX
                                         replace GLYPH's contours with a rectangle
  width FONT.ufo GLYPH ADVANCE
  kern FONT.ufo LEFT RIGHT AMOUNT        glyph names or public.kern1./kern2. groups
  export FONT.ufo OUTPUT.ttf             compile and verify a TrueType font
  status FONT.ufo                        glyphs, widths, kerning, source notes
  recover FONT.ufo                       finish or roll back an interrupted save
Coordinates are font units, y up."
}
fn arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {name}"))
}
fn numeric(args: &[String], index: usize, name: &str) -> Result<f64, String> {
    arg(args, index, name)?
        .parse()
        .map_err(|_| format!("invalid {name}"))
}
fn run(args: &[String]) -> Result<(), String> {
    let command = arg(args, 0, "command")?;
    let path = Path::new(arg(args, 1, "FONT.ufo")?);
    let s = |e: fontstrike::Error| e.to_string();
    match command {
        "new" => {
            FontProject::create(path, arg(args, 2, "family")?, arg(args, 3, "style")?)
                .map_err(s)?;
            println!("Created {}", path.display());
            return Ok(());
        }
        "demo" => {
            fontstrike::demo::build(path).map_err(s)?;
            println!("Created synthetic test font {}", path.display());
            return Ok(());
        }
        "recover" => {
            let report = fontstrike::recover(path).map_err(s)?;
            for action in report.actions {
                println!("{action}");
            }
            return Ok(());
        }
        _ => {}
    }
    let mut project = FontProject::open(path).map_err(s)?;
    match command {
        "rect" => {
            let name = arg(args, 2, "glyph")?;
            let unicode = arg(args, 3, "Unicode character or -")?;
            let codepoints: Vec<char> = if unicode == "-" {
                Vec::new()
            } else {
                let mut chars = unicode.chars();
                let first = chars
                    .next()
                    .ok_or_else(|| "empty Unicode character".to_string())?;
                if chars.next().is_some() {
                    return Err("Unicode must be one character".into());
                }
                vec![first]
            };
            let width = numeric(args, 4, "width")?;
            let (x0, y0, x1, y1) = (
                numeric(args, 5, "x min")?,
                numeric(args, 6, "y min")?,
                numeric(args, 7, "x max")?,
                numeric(args, 8, "y max")?,
            );
            if project.glyph(name).is_none() {
                project.add_glyph(name, &codepoints, width).map_err(s)?;
            } else {
                project.set_codepoints(name, &codepoints).map_err(s)?;
                project.set_width(name, width).map_err(s)?;
                while !project.glyph(name).unwrap().contours.is_empty() {
                    project.delete_contour(name, 0).map_err(s)?;
                }
            }
            project.add_rectangle(name, x0, y0, x1, y1).map_err(s)?;
            project.save().map_err(s)?;
            println!("Saved glyph {name}");
        }
        "width" => {
            project
                .set_width(arg(args, 2, "glyph")?, numeric(args, 3, "advance")?)
                .map_err(s)?;
            project.save().map_err(s)?;
        }
        "kern" => {
            project
                .set_kerning(
                    arg(args, 2, "left")?,
                    arg(args, 3, "right")?,
                    numeric(args, 4, "amount")?,
                )
                .map_err(s)?;
            project.save().map_err(s)?;
        }
        "export" => {
            let output = Path::new(arg(args, 2, "OUTPUT.ttf")?);
            let report = project.export_ttf(output).map_err(s)?;
            println!(
                "Compiled and verified {} ({} bytes)",
                output.display(),
                report.bytes
            );
            println!("  {}", report.summary());
            for note in report.notes {
                println!("  note: {note}");
            }
        }
        "status" => {
            println!(
                "{}: {}",
                path.display(),
                project.family().unwrap_or("Unnamed")
            );
            for name in project.glyph_names() {
                let cps: String = project
                    .codepoints(&name)
                    .map_err(s)?
                    .iter()
                    .map(|c| format!(" U+{:04X}", *c as u32))
                    .collect();
                println!("  {name}: width {}{cps}", project.width(&name).map_err(s)?);
            }
            for (l, r, v) in project.kerning_pairs() {
                println!("  kern {l} {r}: {v}");
            }
            for note in project.notes() {
                println!("  note: {note}");
            }
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{error}\n\n{}", usage());
        std::process::exit(2);
    }
}
