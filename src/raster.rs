//! A small anti-aliased non-zero-winding rasteriser for glyph previews.

use kurbo::{Affine, BezPath, PathEl, Point};

/// 8-bit coverage, row-major, `width * height` bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Coverage {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

const SUBSAMPLES: usize = 4;

/// Fills `path` after applying `transform` (which must map to pixel space,
/// y down) into a `width` x `height` coverage mask.
pub fn rasterize(path: &BezPath, transform: Affine, width: usize, height: usize) -> Coverage {
    let mut edges: Vec<(Point, Point)> = Vec::new();
    let mut start = Point::ZERO;
    let mut last = Point::ZERO;
    let mut transformed = path.clone();
    transformed.apply_affine(transform);
    kurbo::flatten(&transformed, 0.2, |el| match el {
        PathEl::MoveTo(p) => {
            if last != start {
                edges.push((last, start));
            }
            start = p;
            last = p;
        }
        PathEl::LineTo(p) => {
            edges.push((last, p));
            last = p;
        }
        PathEl::ClosePath => {
            if last != start {
                edges.push((last, start));
            }
            last = start;
        }
        _ => {}
    });
    if last != start {
        edges.push((last, start));
    }
    let mut acc = vec![0f32; width * height];
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for row in 0..height {
        for s in 0..SUBSAMPLES {
            let y = row as f64 + (s as f64 + 0.5) / SUBSAMPLES as f64;
            crossings.clear();
            for &(a, b) in &edges {
                if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                    let t = (y - a.y) / (b.y - a.y);
                    let x = a.x + t * (b.x - a.x);
                    crossings.push((x, if b.y > a.y { 1 } else { -1 }));
                }
            }
            crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                if winding != 0 {
                    span(
                        &mut acc[row * width..(row + 1) * width],
                        pair[0].0,
                        pair[1].0,
                    );
                }
            }
        }
    }
    let alpha = acc
        .into_iter()
        .map(|c| ((c / SUBSAMPLES as f32).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    Coverage {
        width,
        height,
        alpha,
    }
}

fn span(row: &mut [f32], x0: f64, x1: f64) {
    let w = row.len() as f64;
    let (x0, x1) = (x0.clamp(0.0, w), x1.clamp(0.0, w));
    if x1 <= x0 {
        return;
    }
    let (i0, i1) = (x0.floor() as usize, x1.floor() as usize);
    if i0 == i1 {
        if i0 < row.len() {
            row[i0] += (x1 - x0) as f32;
        }
        return;
    }
    row[i0] += (1.0 - (x0 - i0 as f64)) as f32;
    for cell in row.iter_mut().take(i1).skip(i0 + 1) {
        *cell += 1.0;
    }
    if i1 < row.len() {
        row[i1] += (x1 - i1 as f64) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Rect, Shape};

    #[test]
    fn square_with_counter_uses_nonzero_winding() {
        let mut path = Rect::new(2.0, 2.0, 18.0, 18.0).to_path(0.1);
        // Reverse-wound inner square makes a hole.
        let inner = Rect::new(6.0, 6.0, 14.0, 14.0).to_path(0.1);
        let mut reversed = BezPath::new();
        let pts: Vec<Point> = inner
            .elements()
            .iter()
            .filter_map(|e| match e {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect();
        reversed.move_to(pts[0]);
        for p in pts.iter().rev() {
            reversed.line_to(*p);
        }
        reversed.close_path();
        path.extend(reversed);
        let c = rasterize(&path, Affine::IDENTITY, 20, 20);
        assert_eq!(c.alpha[3 * 20 + 3], 255);
        assert_eq!(c.alpha[10 * 20 + 10], 0);
        assert_eq!(c.alpha[0], 0);
    }
}
