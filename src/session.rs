//! UI-independent document controller: open/new/save/export flows,
//! cancelled dialogs, unsaved-change guards, recovery, and status notices.
//! The desktop app drives this; tests exercise it without a window.

use crate::{ExportReport, FontProject, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub level: Level,
    pub text: String,
}

impl Notice {
    fn new(level: Level, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }
}

/// Result of trying to open a UFO.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenOutcome {
    /// The dialog was cancelled; nothing changed.
    Cancelled,
    /// Refused because the current font has unsaved edits.
    Blocked,
    Opened,
    /// An interrupted save must be recovered first (description inside).
    NeedsRecovery(String),
    /// Unsaved edits from a crashed session exist; ask restore or discard.
    AutosaveAvailable,
    Failed,
}

pub struct Session {
    pub project: Option<FontProject>,
    pub notice: Notice,
    pub last_export: Option<PathBuf>,
    pub last_report: Option<ExportReport>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            project: None,
            notice: Notice::new(Level::Info, "Create a new UFO or open an existing one"),
            last_export: None,
            last_report: None,
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.project.as_ref().is_some_and(FontProject::is_dirty)
    }

    fn set(&mut self, level: Level, text: impl Into<String>) {
        self.notice = Notice::new(level, text);
    }

    fn guard(&mut self, discard_unsaved: bool) -> bool {
        if self.has_unsaved() && !discard_unsaved {
            self.set(Level::Warning, "Save or discard your unsaved changes first");
            return false;
        }
        true
    }

    /// `path` is the save-dialog result; `None` means the user cancelled.
    pub fn new_font(
        &mut self,
        path: Option<PathBuf>,
        family: &str,
        style: &str,
        discard_unsaved: bool,
    ) -> bool {
        if !self.guard(discard_unsaved) {
            return false;
        }
        let Some(mut path) = path else {
            self.set(Level::Info, "New font cancelled; nothing changed");
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("ufo");
        }
        match FontProject::create(&path, family, style) {
            Ok(p) => {
                self.set(Level::Success, format!("Created {}", display_name(&path)));
                self.project = Some(p);
                true
            }
            Err(e) => {
                self.set(Level::Error, format!("Could not create font: {e}"));
                false
            }
        }
    }

    /// `path` is the folder-dialog result; `None` means the user cancelled.
    pub fn open_font(&mut self, path: Option<PathBuf>, discard_unsaved: bool) -> OpenOutcome {
        if !self.guard(discard_unsaved) {
            return OpenOutcome::Blocked;
        }
        let Some(path) = path else {
            self.set(Level::Info, "Open cancelled; nothing changed");
            return OpenOutcome::Cancelled;
        };
        if let Some(problem) = crate::pending_recovery(&path) {
            self.set(Level::Warning, format!("{problem}. Recover to continue."));
            return OpenOutcome::NeedsRecovery(problem);
        }
        if FontProject::autosave_available(&path) {
            self.set(
                Level::Warning,
                "Unsaved edits from an earlier session were found",
            );
            return OpenOutcome::AutosaveAvailable;
        }
        self.finish_open(&path, FontProject::open(&path))
    }

    fn finish_open(&mut self, path: &Path, result: Result<FontProject>) -> OpenOutcome {
        match result {
            Ok(p) => {
                let notes = p.notes().len();
                self.set(
                    if notes > 0 {
                        Level::Warning
                    } else {
                        Level::Success
                    },
                    if notes > 0 {
                        format!(
                            "Opened {} with {notes} source notes (see Source notes)",
                            display_name(path)
                        )
                    } else {
                        format!("Opened {}", display_name(path))
                    },
                );
                self.project = Some(p);
                OpenOutcome::Opened
            }
            Err(e) => {
                self.set(
                    Level::Error,
                    format!("Could not open {}: {e}", display_name(path)),
                );
                OpenOutcome::Failed
            }
        }
    }

    /// Answers [`OpenOutcome::AutosaveAvailable`].
    pub fn resolve_autosave(&mut self, path: &Path, restore: bool) -> OpenOutcome {
        if restore {
            self.finish_open(path, FontProject::open_autosave(path))
        } else {
            if let Err(e) = FontProject::discard_autosave(path) {
                self.set(Level::Error, format!("Could not discard autosave: {e}"));
                return OpenOutcome::Failed;
            }
            self.finish_open(path, FontProject::open(path))
        }
    }

    /// Answers [`OpenOutcome::NeedsRecovery`]: recovers, then opens.
    pub fn recover_and_open(&mut self, path: &Path) -> OpenOutcome {
        match crate::recover(path) {
            Ok(report) => {
                let outcome = self.open_font(Some(path.to_path_buf()), true);
                if outcome == OpenOutcome::Opened {
                    self.set(
                        Level::Success,
                        format!("Recovered: {}", report.actions.join("; ")),
                    );
                }
                outcome
            }
            Err(e) => {
                self.set(Level::Error, format!("Recovery failed: {e}"));
                OpenOutcome::Failed
            }
        }
    }

    pub fn save(&mut self) -> bool {
        let Some(p) = self.project.as_mut() else {
            self.set(Level::Info, "Nothing to save");
            return false;
        };
        match p.save() {
            Ok(()) => {
                let name = display_name(p.root());
                self.set(Level::Success, format!("Saved {name}"));
                true
            }
            Err(e) => {
                self.set(Level::Error, format!("Not saved: {e}"));
                false
            }
        }
    }

    pub fn save_as(&mut self, path: Option<PathBuf>) -> bool {
        let Some(mut path) = path else {
            self.set(Level::Info, "Save As cancelled; nothing changed");
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("ufo");
        }
        let Some(p) = self.project.as_mut() else {
            return false;
        };
        match p.save_as(&path) {
            Ok(()) => {
                self.set(Level::Success, format!("Saved as {}", display_name(&path)));
                true
            }
            Err(e) => {
                self.set(Level::Error, format!("Not saved: {e}"));
                false
            }
        }
    }

    /// Records a finished export (`None` path means the dialog was cancelled).
    pub fn finish_export(
        &mut self,
        path: Option<&Path>,
        result: Option<Result<ExportReport>>,
    ) -> bool {
        let (Some(path), Some(result)) = (path, result) else {
            self.set(Level::Info, "Export cancelled; nothing was written");
            return false;
        };
        match result {
            Ok(report) => {
                self.set(
                    Level::Success,
                    format!("Exported {}: {}", display_name(path), report.summary()),
                );
                self.last_export = Some(path.to_path_buf());
                self.last_report = Some(report);
                true
            }
            Err(e) => {
                self.set(
                    Level::Error,
                    format!("Export failed, previous file untouched: {e}"),
                );
                false
            }
        }
    }

    /// Synchronous export (CLI and tests).
    pub fn export_now(&mut self, path: Option<PathBuf>) -> bool {
        let result = match (&self.project, &path) {
            (Some(p), Some(path)) => Some(p.export_ttf(path)),
            (None, Some(_)) => {
                self.set(Level::Info, "Open a font before exporting");
                return false;
            }
            _ => None,
        };
        self.finish_export(path.as_deref(), result)
    }

    pub fn close(&mut self, discard_unsaved: bool) -> bool {
        if !self.guard(discard_unsaved) {
            return false;
        }
        if let Some(p) = self.project.take()
            && discard_unsaved
        {
            let _ = FontProject::discard_autosave(p.root());
        }
        self.set(Level::Info, "Font closed");
        true
    }
}

pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
