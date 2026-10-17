//! Point-level contour editing on UFO data.
//!
//! The editor works directly on `norad` contour points (font units, y up) so
//! that point names, identifiers, smooth flags, quadratic (`qcurve`) segments,
//! and libs survive every edit that does not explicitly remove them.

use crate::{Error, Result};
use kurbo::{Affine, BezPath, CubicBez, Line, ParamCurve, PathSeg, Point, QuadBez, Rect, Shape};
use norad::{Contour, ContourPoint, Font, Glyph, PointType};

/// Identifies one point inside a glyph's contour list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointId {
    pub contour: usize,
    pub point: usize,
}

impl PointId {
    pub fn new(contour: usize, point: usize) -> Self {
        Self { contour, point }
    }
}

/// The geometry family of one segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Line,
    Cubic,
    /// A quadratic segment with a single off-curve point.
    Quadratic,
    /// A TrueType-style quadratic run with implied on-curve points. Point
    /// positions are editable; splitting it is not supported.
    QuadraticRun,
}

/// One segment of a contour, named by the index of its ending on-curve point.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub start: usize,
    pub offs: Vec<usize>,
    pub end: usize,
    pub kind: SegmentKind,
}

pub(crate) fn is_on(p: &ContourPoint) -> bool {
    p.typ != PointType::OffCurve
}

fn pt(p: &ContourPoint) -> Point {
    Point::new(p.x, p.y)
}

fn new_point(at: Point, typ: PointType, smooth: bool) -> ContourPoint {
    ContourPoint::new(at.x, at.y, typ, smooth, None, None)
}

/// Lists the segments of a contour in drawing order.
pub fn segments(contour: &Contour) -> Vec<Segment> {
    let points = &contour.points;
    let on: Vec<usize> = (0..points.len()).filter(|&i| is_on(&points[i])).collect();
    if on.is_empty() {
        return Vec::new();
    }
    let closed = contour.is_closed();
    let mut out = Vec::new();
    let n = points.len();
    let pairs: Vec<(usize, usize)> = if closed {
        (0..on.len())
            .map(|k| (on[(k + on.len() - 1) % on.len()], on[k]))
            .collect()
    } else {
        on.windows(2).map(|w| (w[0], w[1])).collect()
    };
    for (start, end) in pairs {
        if closed && on.len() == 1 && start == end && n == 1 {
            continue;
        }
        let mut offs = Vec::new();
        let mut i = (start + 1) % n;
        while i != end {
            offs.push(i);
            i = (i + 1) % n;
        }
        let kind = match (points[end].typ, offs.len()) {
            (_, 0) => SegmentKind::Line,
            (PointType::Curve, 1) | (PointType::QCurve, 1) => SegmentKind::Quadratic,
            (PointType::Curve, 2) => SegmentKind::Cubic,
            (PointType::QCurve, _) => SegmentKind::QuadraticRun,
            _ => SegmentKind::QuadraticRun,
        };
        out.push(Segment {
            start,
            offs,
            end,
            kind,
        });
    }
    out
}

/// The drawable geometry of a simple segment.
pub fn segment_geometry(contour: &Contour, seg: &Segment) -> Option<PathSeg> {
    let p = &contour.points;
    let a = pt(&p[seg.start]);
    let b = pt(&p[seg.end]);
    match seg.kind {
        SegmentKind::Line => Some(PathSeg::Line(Line::new(a, b))),
        SegmentKind::Quadratic => Some(PathSeg::Quad(QuadBez::new(a, pt(&p[seg.offs[0]]), b))),
        SegmentKind::Cubic => Some(PathSeg::Cubic(CubicBez::new(
            a,
            pt(&p[seg.offs[0]]),
            pt(&p[seg.offs[1]]),
            b,
        ))),
        SegmentKind::QuadraticRun => None,
    }
}

/// Runs `edit` on the contour's points rotated so `start` is first, then
/// restores the original first point (if it still exists) to the front.
fn with_rotation(
    contour: &mut Contour,
    start: usize,
    edit: impl FnOnce(&mut Vec<(ContourPoint, bool)>) -> Result<()>,
) -> Result<usize> {
    let mut tagged: Vec<(ContourPoint, bool)> = contour
        .points
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, p)| (p, i == 0))
        .collect();
    tagged.rotate_left(start);
    edit(&mut tagged)?;
    let shift = tagged.iter().position(|(_, f)| *f).unwrap_or(0);
    tagged.rotate_left(shift);
    contour.points = tagged.into_iter().map(|(p, _)| p).collect();
    Ok(shift)
}

fn segment_ending_at(contour: &Contour, end: usize) -> Result<Segment> {
    segments(contour)
        .into_iter()
        .find(|s| s.end == end)
        .ok_or_else(|| Error::new("that point does not end a segment"))
}

/// Splits the segment that ends at `end` at parameter `t`, returning the index
/// of the new on-curve point.
pub fn insert_point(contour: &mut Contour, end: usize, t: f64) -> Result<usize> {
    let t = t.clamp(0.01, 0.99);
    let seg = segment_ending_at(contour, end)?;
    let geometry = segment_geometry(contour, &seg)
        .ok_or_else(|| Error::new("splitting a multi-point quadratic run is not supported"))?;
    let end_type = contour.points[seg.end].typ;
    let replacement: Vec<ContourPoint> = match geometry {
        PathSeg::Line(line) => vec![new_point(line.eval(t), PointType::Line, false)],
        PathSeg::Quad(q) => {
            let (a, b) = (q.subsegment(0.0..t), q.subsegment(t..1.0));
            vec![
                new_point(a.p1, PointType::OffCurve, false),
                new_point(a.p2, end_type, true),
                new_point(b.p1, PointType::OffCurve, false),
            ]
        }
        PathSeg::Cubic(c) => {
            let (a, b) = (c.subsegment(0.0..t), c.subsegment(t..1.0));
            vec![
                new_point(a.p1, PointType::OffCurve, false),
                new_point(a.p2, PointType::OffCurve, false),
                new_point(a.p3, PointType::Curve, true),
                new_point(b.p1, PointType::OffCurve, false),
                new_point(b.p2, PointType::OffCurve, false),
            ]
        }
    };
    let offs = seg.offs.len();
    let rel_new = match geometry {
        PathSeg::Line(_) => 1,
        PathSeg::Quad(_) => 2,
        PathSeg::Cubic(_) => 3,
    };
    let shift = with_rotation(contour, seg.start, |pts| {
        pts.drain(1..1 + offs);
        for (k, p) in replacement.into_iter().enumerate() {
            pts.insert(1 + k, (p, false));
        }
        Ok(())
    })?;
    let len = contour.points.len();
    Ok((rel_new + len - shift % len) % len)
}

/// Deletes points. On-curve deletion merges neighbouring segments, keeping the
/// outer handles; off-curve deletion turns its segment into a line. Returns
/// `true` when the contour became empty and should be removed.
pub fn delete_point(contour: &mut Contour, index: usize) -> Result<bool> {
    let n = contour.points.len();
    if index >= n {
        return Err(Error::new("point index out of range"));
    }
    let closed = contour.is_closed();
    if !is_on(&contour.points[index]) {
        let seg = segments(contour)
            .into_iter()
            .find(|s| s.offs.contains(&index))
            .ok_or_else(|| Error::new("off-curve point is not attached to a segment"))?;
        let mut remove = seg.offs.clone();
        remove.sort_unstable();
        let end = seg.end;
        contour.points[end].typ = PointType::Line;
        contour.points[end].smooth = false;
        for i in remove.into_iter().rev() {
            contour.points.remove(i);
        }
        return Ok(false);
    }
    let on_count = contour.points.iter().filter(|p| is_on(p)).count();
    if on_count <= 2 {
        contour.points.clear();
        return Ok(true);
    }
    let segs = segments(contour);
    if !closed {
        let first_on = segs.first().map(|s| s.start);
        let last_on = segs.last().map(|s| s.end);
        if Some(index) == first_on {
            let seg = &segs[0];
            let next = seg.end;
            let mut remove = vec![index];
            remove.extend(seg.offs.iter().copied());
            contour.points[next].typ = PointType::Move;
            contour.points[next].smooth = false;
            remove.sort_unstable();
            for i in remove.into_iter().rev() {
                contour.points.remove(i);
            }
            return Ok(false);
        }
        if Some(index) == last_on {
            let seg = segs.last().unwrap();
            let mut remove = vec![index];
            remove.extend(seg.offs.iter().copied());
            remove.sort_unstable();
            for i in remove.into_iter().rev() {
                contour.points.remove(i);
            }
            return Ok(false);
        }
    }
    let incoming = segs
        .iter()
        .find(|s| s.end == index)
        .cloned()
        .ok_or_else(|| Error::new("point has no incoming segment"))?;
    let outgoing = segs
        .iter()
        .find(|s| s.start == index)
        .cloned()
        .ok_or_else(|| Error::new("point has no outgoing segment"))?;
    let points = contour.points.clone();
    let a = pt(&points[incoming.start]);
    let b = pt(&points[outgoing.end]);
    let quadratic = |s: &Segment| {
        matches!(s.kind, SegmentKind::QuadraticRun)
            || points[s.end].typ == PointType::QCurve && !s.offs.is_empty()
    };
    let (new_offs, end_type): (Vec<ContourPoint>, PointType) =
        if incoming.offs.is_empty() && outgoing.offs.is_empty() {
            (Vec::new(), PointType::Line)
        } else if quadratic(&incoming) || quadratic(&outgoing) {
            if (!incoming.offs.is_empty() && !quadratic(&incoming))
                || (!outgoing.offs.is_empty() && !quadratic(&outgoing))
            {
                return Err(Error::new(
                    "cannot merge a cubic and a quadratic segment; convert one first",
                ));
            }
            let offs = incoming
                .offs
                .iter()
                .chain(&outgoing.offs)
                .map(|&i| points[i].clone())
                .collect();
            (offs, PointType::QCurve)
        } else {
            let first = incoming
                .offs
                .first()
                .map(|&i| points[i].clone())
                .unwrap_or_else(|| new_point(a, PointType::OffCurve, false));
            let last = outgoing
                .offs
                .last()
                .map(|&i| points[i].clone())
                .unwrap_or_else(|| new_point(b, PointType::OffCurve, false));
            (vec![first, last], PointType::Curve)
        };
    let removed = incoming.offs.len() + 1 + outgoing.offs.len();
    with_rotation(contour, incoming.start, |pts| {
        pts.drain(1..1 + removed);
        for (k, p) in new_offs.into_iter().enumerate() {
            pts.insert(1 + k, (p, false));
        }
        let end = pts
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, (p, _))| is_on(p))
            .map(|(i, _)| i)
            .ok_or_else(|| Error::new("contour lost its end point"))?;
        pts[end].0.typ = end_type;
        if end_type == PointType::Line {
            pts[end].0.smooth = false;
        }
        Ok(())
    })?;
    Ok(false)
}

/// Converts a segment between straight and cubic.
pub fn convert_segment(contour: &mut Contour, end: usize, to_curve: bool) -> Result<()> {
    let seg = segment_ending_at(contour, end)?;
    let a = pt(&contour.points[seg.start]);
    let b = pt(&contour.points[seg.end]);
    if to_curve {
        if seg.kind != SegmentKind::Line {
            return Ok(());
        }
        contour.points[seg.end].typ = PointType::Curve;
        let c1 = new_point(a.lerp(b, 1.0 / 3.0), PointType::OffCurve, false);
        let c2 = new_point(a.lerp(b, 2.0 / 3.0), PointType::OffCurve, false);
        with_rotation(contour, seg.start, |pts| {
            pts.insert(1, (c2, false));
            pts.insert(1, (c1, false));
            Ok(())
        })
        .map(|_| ())
    } else {
        if seg.kind == SegmentKind::Line {
            return Ok(());
        }
        let mut offs = seg.offs.clone();
        offs.sort_unstable();
        contour.points[seg.end].typ = PointType::Line;
        contour.points[seg.end].smooth = false;
        for i in offs.into_iter().rev() {
            contour.points.remove(i);
        }
        Ok(())
    }
}

/// Reverses contour direction, keeping the first on-curve point first.
pub fn reverse_contour(contour: &mut Contour) -> Result<()> {
    let segs = segments(contour);
    if segs.is_empty() {
        return Err(Error::new("contour has no on-curve points"));
    }
    let p = contour.points.clone();
    let mut out: Vec<ContourPoint> = Vec::with_capacity(p.len());
    let typed = |src: &ContourPoint, typ: PointType| {
        let mut q = src.clone();
        q.typ = typ;
        if typ == PointType::Line || typ == PointType::Move {
            q.smooth = src.smooth && typ != PointType::Move;
        }
        q
    };
    if contour.is_closed() {
        // segs[k] ends at on-curve k (in drawing order); rotate so segment 0
        // ends at the first on-curve point in storage order.
        let first_on = segs.iter().map(|s| s.end).min().unwrap();
        let k0 = segs.iter().position(|s| s.end == first_on).unwrap();
        let m = segs.len();
        let seg = |k: usize| &segs[(k0 + k) % m];
        // Q0 = P0 gets the type of segment 1 (P0 -> P1 reversed).
        out.push(typed(&p[seg(0).end], p[seg(1 % m).end].typ));
        for j in 1..m {
            // Segment arriving at Q_j is the reverse of S_{m-j+1}.
            let s = seg((m - j + 1) % m);
            for &o in s.offs.iter().rev() {
                out.push(p[o].clone());
            }
            out.push(typed(&p[s.start], p[s.end].typ));
        }
        for &o in seg(1 % m).offs.iter().rev() {
            out.push(p[o].clone());
        }
    } else {
        let last = segs.last().unwrap();
        out.push(typed(&p[last.end], PointType::Move));
        for s in segs.iter().rev() {
            for &o in s.offs.iter().rev() {
                out.push(p[o].clone());
            }
            out.push(typed(&p[s.start], p[s.end].typ));
        }
    }
    contour.points = out;
    Ok(())
}

/// Builds a closed contour from on-curve corner points in font units.
pub fn polygon(points: &[(f64, f64)]) -> Result<Contour> {
    if points.len() < 3 {
        return Err(Error::new("a closed contour needs at least three points"));
    }
    check_coords(points.iter().copied())?;
    Ok(Contour::new(
        points
            .iter()
            .map(|&(x, y)| new_point(Point::new(x, y), PointType::Line, false))
            .collect(),
        None,
    ))
}

/// Builds a closed four-segment cubic ellipse.
pub fn ellipse(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Contour> {
    if x0 >= x1 || y0 >= y1 {
        return Err(Error::new("ellipse min must be below max"));
    }
    check_coords([(x0, y0), (x1, y1)])?;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    const K: f64 = 0.552_284_749_8;
    let on = |x, y| new_point(Point::new(x, y), PointType::Curve, true);
    let off = |x, y| new_point(Point::new(x, y), PointType::OffCurve, false);
    // Counter-clockwise outer contour (PostScript/UFO convention for cubic outlines).
    Ok(Contour::new(
        vec![
            on(cx, y0),
            off(cx + rx * K, y0),
            off(x1, cy - ry * K),
            on(x1, cy),
            off(x1, cy + ry * K),
            off(cx + rx * K, y1),
            on(cx, y1),
            off(cx - rx * K, y1),
            off(x0, cy + ry * K),
            on(x0, cy),
            off(x0, cy - ry * K),
            off(cx - rx * K, y0),
        ],
        None,
    ))
}

pub(crate) fn check_coords(points: impl IntoIterator<Item = (f64, f64)>) -> Result<()> {
    for (x, y) in points {
        if !x.is_finite() || !y.is_finite() || x.abs() > 32_000.0 || y.abs() > 32_000.0 {
            return Err(Error::new(
                "coordinates must be finite and within ±32000 units",
            ));
        }
    }
    Ok(())
}

pub(crate) fn transform_to_affine(t: &norad::AffineTransform) -> Affine {
    Affine::new([
        t.x_scale, t.xy_scale, t.yx_scale, t.y_scale, t.x_offset, t.y_offset,
    ])
}

/// The glyph's own contours as one path (font units, y up).
pub fn contours_path(glyph: &Glyph) -> BezPath {
    let mut path = BezPath::new();
    for contour in &glyph.contours {
        if let Ok(sub) = contour.to_kurbo() {
            let closed = contour.is_closed();
            path.extend(sub.elements().iter().copied());
            if closed {
                path.close_path();
            }
        }
    }
    path
}

/// Contours plus resolved components (default layer), font units, y up.
pub fn resolved_path(font: &Font, glyph: &Glyph) -> BezPath {
    let mut path = contours_path(glyph);
    append_components(font, glyph, Affine::IDENTITY, 0, &mut path);
    path
}

fn append_components(font: &Font, glyph: &Glyph, outer: Affine, depth: usize, out: &mut BezPath) {
    if depth > 12 {
        return;
    }
    for component in &glyph.components {
        let Some(base) = font.default_layer().get_glyph(&component.base) else {
            continue;
        };
        let t = outer * transform_to_affine(&component.transform);
        let mut sub = contours_path(base);
        sub.apply_affine(t);
        out.extend(sub.elements().iter().copied());
        append_components(font, base, t, depth + 1, out);
    }
}

/// Tight bounds of the resolved outline, or `None` for an empty glyph.
pub fn bounds(font: &Font, glyph: &Glyph) -> Option<Rect> {
    let path = resolved_path(font, glyph);
    if path.elements().is_empty() {
        None
    } else {
        Some(path.bounding_box())
    }
}

/// True when every contour segment is straight.
pub fn line_only(glyph: &Glyph) -> bool {
    glyph
        .contours
        .iter()
        .all(|c| c.points.iter().all(|p| p.typ != PointType::OffCurve))
}

/// Nearest point on any segment to `target`, as (contour, segment end, t, distance).
pub fn nearest_segment(glyph: &Glyph, target: Point) -> Option<(usize, usize, f64, f64)> {
    let mut best: Option<(usize, usize, f64, f64)> = None;
    for (ci, contour) in glyph.contours.iter().enumerate() {
        for seg in segments(contour) {
            let Some(geometry) = segment_geometry(contour, &seg) else {
                continue;
            };
            let mut t_best = 0.0;
            let mut d_best = f64::MAX;
            for i in 0..=48 {
                let t = i as f64 / 48.0;
                let d = geometry.eval(t).distance(target);
                if d < d_best {
                    d_best = d;
                    t_best = t;
                }
            }
            // Local refinement.
            let mut step = 1.0 / 96.0;
            for _ in 0..12 {
                for cand in [t_best - step, t_best + step] {
                    if (0.0..=1.0).contains(&cand) {
                        let d = geometry.eval(cand).distance(target);
                        if d < d_best {
                            d_best = d;
                            t_best = cand;
                        }
                    }
                }
                step /= 2.0;
            }
            if best.is_none_or(|b| d_best < b.3) {
                best = Some((ci, seg.end, t_best, d_best));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Contour {
        polygon(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]).unwrap()
    }

    #[test]
    fn insert_on_line_and_curve_keep_shape() {
        let mut c = square();
        let idx = insert_point(&mut c, 1, 0.5).unwrap();
        assert_eq!(c.points.len(), 5);
        assert_eq!((c.points[idx].x, c.points[idx].y), (50.0, 0.0));
        let mut e = ellipse(0.0, 0.0, 200.0, 100.0).unwrap();
        let before = contours_path(&glyph_with(e.clone())).bounding_box();
        insert_point(&mut e, 3, 0.5).unwrap();
        assert_eq!(e.points.len(), 15);
        let after = contours_path(&glyph_with(e)).bounding_box();
        assert!((before.x1 - after.x1).abs() < 1e-6 && (before.y0 - after.y0).abs() < 1e-6);
    }

    fn glyph_with(c: Contour) -> Glyph {
        let mut g = Glyph::new("t");
        g.contours.push(c);
        g
    }

    #[test]
    fn delete_merges_and_off_curve_deletion_straightens() {
        let mut e = ellipse(0.0, 0.0, 200.0, 100.0).unwrap();
        assert!(!delete_point(&mut e, 3).unwrap());
        assert_eq!(e.points.iter().filter(|p| is_on(p)).count(), 3);
        assert_eq!(e.points.len(), 9);
        let mut e2 = ellipse(0.0, 0.0, 200.0, 100.0).unwrap();
        delete_point(&mut e2, 1).unwrap();
        assert_eq!(e2.points[1].typ, PointType::Line);
        let mut s = square();
        delete_point(&mut s, 0).unwrap();
        assert_eq!(s.points.len(), 3);
        assert!(delete_point(&mut s, 0).unwrap() || s.points.len() == 2);
    }

    #[test]
    fn reverse_twice_is_identity_and_flips_area() {
        let e = ellipse(0.0, 0.0, 200.0, 100.0).unwrap();
        let area = contours_path(&glyph_with(e.clone())).area();
        let mut r = e.clone();
        reverse_contour(&mut r).unwrap();
        let area_r = contours_path(&glyph_with(r.clone())).area();
        assert!((area + area_r).abs() < 1e-6, "{area} {area_r}");
        reverse_contour(&mut r).unwrap();
        assert_eq!(r, e);
    }

    #[test]
    fn convert_segment_roundtrip() {
        let mut s = square();
        convert_segment(&mut s, 2, true).unwrap();
        assert_eq!(s.points.len(), 6);
        assert_eq!(
            segments(&s)
                .iter()
                .filter(|g| g.kind == SegmentKind::Cubic)
                .count(),
            1
        );
        let end = segments(&s)
            .iter()
            .find(|g| g.kind == SegmentKind::Cubic)
            .unwrap()
            .end;
        convert_segment(&mut s, end, false).unwrap();
        assert_eq!(s, square());
    }
}
