use fontstrike::{
    FontProject,
    vector_path::{Anchor, PathData, SubPath},
};
use kurbo::Point;
use std::path::Path;

fn usage() -> &'static str {
    "FontStrike commands:\n  new FAMILY.ufo FAMILY STYLE\n  rect FONT.ufo GLYPH UNICODE|- WIDTH X_MIN Y_MIN X_MAX Y_MAX\n  width FONT.ufo GLYPH ADVANCE\n  kern FONT.ufo LEFT RIGHT AMOUNT\n  export FONT.ufo OUTPUT.ttf\n  status FONT.ufo\nCoordinates for rect are conventional font units, y up."
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
    if command == "new" {
        FontProject::create(path, arg(args, 2, "family")?, arg(args, 3, "style")?)
            .map_err(|e| e.to_string())?;
        println!("Created {}", path.display());
        return Ok(());
    }
    let mut project = FontProject::open(path).map_err(|e| e.to_string())?;
    match command {
        "rect" => {
            let name = arg(args, 2, "glyph")?;
            let unicode = arg(args, 3, "Unicode character or -")?;
            let codepoint = if unicode == "-" {
                None
            } else {
                let mut chars = unicode.chars();
                let first = chars
                    .next()
                    .ok_or_else(|| "empty Unicode character".to_string())?;
                if chars.next().is_some() {
                    return Err("Unicode must be one character".into());
                }
                Some(first)
            };
            let width = numeric(args, 4, "width")?;
            let x0 = numeric(args, 5, "x min")?;
            let y0 = numeric(args, 6, "y min")?;
            let x1 = numeric(args, 7, "x max")?;
            let y1 = numeric(args, 8, "y max")?;
            if x0 >= x1 || y0 >= y1 {
                return Err("rectangle min must be below max".into());
            }
            let points = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
            let path = PathData::single(SubPath::new(
                points
                    .into_iter()
                    .map(|(x, y)| Anchor::corner(Point::new(x, -y)))
                    .collect(),
                true,
            ));
            project
                .set_outline(name, codepoint, width, &path)
                .map_err(|e| e.to_string())?;
            project.save().map_err(|e| e.to_string())?;
            println!("Saved glyph {name}");
        }
        "width" => {
            project
                .set_width(arg(args, 2, "glyph")?, numeric(args, 3, "advance")?)
                .map_err(|e| e.to_string())?;
            project.save().map_err(|e| e.to_string())?;
        }
        "kern" => {
            project
                .set_kerning(
                    arg(args, 2, "left")?,
                    arg(args, 3, "right")?,
                    numeric(args, 4, "amount")?,
                )
                .map_err(|e| e.to_string())?;
            project.save().map_err(|e| e.to_string())?;
        }
        "export" => {
            let output = Path::new(arg(args, 2, "OUTPUT.ttf")?);
            project.export_ttf(output).map_err(|e| e.to_string())?;
            println!("Compiled and checked {}", output.display());
        }
        "status" => {
            println!(
                "{}: {}",
                path.display(),
                project.family().unwrap_or("Unnamed")
            );
            for name in project.glyph_names() {
                println!(
                    "  {name}: width {}",
                    project.width(&name).map_err(|e| e.to_string())?
                );
            }
        }
        _ => return Err(usage().into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{error}\n{}", usage());
        std::process::exit(2);
    }
}
