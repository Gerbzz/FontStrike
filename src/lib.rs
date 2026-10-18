//! FontStrike: an editable UFO font source with lossless saves, undo/redo,
//! and verified TrueType export.
//!
//! * [`FontProject`] owns the UFO model, every edit, and the undo history.
//! * [`storage`] implements crash-safe saves, recovery, and autosave.
//! * [`export`] compiles with fontc and independently verifies the result.
//! * [`outline`] edits contour points directly on UFO data.
//! * [`session`] is the UI-independent controller used by the desktop app.

use kurbo::Point;
use norad::{Contour, ContourPoint, PointType};

pub mod demo;
pub mod export;
pub mod outline;
mod project;
pub mod raster;
pub mod session;
pub mod storage;
pub mod theme;

pub use export::{ExportJob, ExportReport};
pub use norad;
pub use outline::PointId;
pub use project::{FontMetrics, FontProject};
pub use storage::{RecoveryReport, SaveInterruption, pending_recovery, recover};

pub const EPS: f64 = 1e-9;
#[rustfmt::skip] // donor file is kept byte-identical (see THIRD_PARTY.md)
#[path = "../third_party/vectorcraft/path.rs"]
pub mod vector_path;

/// A user-presentable error.
#[derive(Debug, Clone, PartialEq)]
pub struct Error(String);
impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

fn cp(x: f64, y: f64, typ: PointType) -> ContourPoint {
    // The VectorCraft model is y-down; UFO is y-up.
    ContourPoint::new(x, -y, typ, false, None, None)
}

/// Converts closed VectorCraft subpaths (y down) to UFO contours (y up).
pub(crate) fn to_ufo_contours(path: &vector_path::PathData) -> Result<Vec<Contour>> {
    let mut contours = Vec::with_capacity(path.subpaths.len());
    for sub in &path.subpaths {
        let anchors = &sub.anchors;
        if !sub.closed || anchors.len() < 3 {
            return Err(Error::new("open or short contour"));
        }
        let all: Vec<Point> = anchors
            .iter()
            .flat_map(|a| [a.p, a.h_in, a.h_out])
            .collect();
        outline::check_coords(all.iter().map(|p| (p.x, p.y)))?;
        let last = anchors.last().unwrap();
        let first = &anchors[0];
        let close_curve = last.has_out() || first.has_in();
        let mut points = vec![cp(
            first.p.x,
            first.p.y,
            if close_curve {
                PointType::Curve
            } else {
                PointType::Line
            },
        )];
        for pair in anchors.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.has_out() || b.has_in() {
                points.push(cp(a.h_out.x, a.h_out.y, PointType::OffCurve));
                points.push(cp(b.h_in.x, b.h_in.y, PointType::OffCurve));
                points.push(cp(b.p.x, b.p.y, PointType::Curve));
            } else {
                points.push(cp(b.p.x, b.p.y, PointType::Line));
            }
        }
        if close_curve {
            points.push(cp(last.h_out.x, last.h_out.y, PointType::OffCurve));
            points.push(cp(first.h_in.x, first.h_in.y, PointType::OffCurve));
        }
        contours.push(Contour::new(points, None));
    }
    Ok(contours)
}
