//! Library export + paste-a-list import dialogs (v0.3 program
//! #6 + #7) — a fourth `impl FaveApp` block, split out of `app.rs`
//! for the same reason as `app_share.rs`: that file is near the
//! publish-size ceiling.
//!
//! Both dialogs only collect intent and dispatch through the command
//! registry (`export.library`, `import.paste`), exactly like the
//! rest of the GUI. The paste dialog's "Review matches" step runs
//! the existing provider search per pasted line on a background
//! thread (drinks lines match locally against the bundled database,
//! like the add flow), so what the review promises is what the
//! import does: matched rows carry the provider's fields and
//! identity; unmatched rows add title-only; rows already in the
//! destination are marked and start unchecked.

use crate::app::{FaveApp, COL_ACCENT, COL_MUTED};
use crate::model::{Category, Item};
use eframe::egui;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver};

// ---------------------------------------------------------------------------
// Library export (#6)
// ---------------------------------------------------------------------------

/// Library-export dialog state: the format choice and the last
/// export's error (success reports in the status line instead).
#[derive(Default)]
pub(crate) struct LibraryExportState {
    pub open: bool,
    pub format_json: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Paste-a-list import (#7)
// ---------------------------------------------------------------------------

/// A provider match for one pasted line: everything the import row
/// needs, plus the display line the review shows.
pub(crate) struct PasteMatch {
    pub title: String,
    pub detail: String,
    pub fields: BTreeMap<String, String>,
    pub provider_id: String,
    pub source: String,
}

/// How one pasted line will be imported, per the review.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteRowKind {
    /// A provider (or drinks-database) match was found.
    Matched,
    /// No match: the line adds as a title-only manual entry.
    Manual,
    /// The work is already in the destination (or repeats an
    /// earlier line): skipped by the import; starts unchecked.
    AlreadyPresent,
}

/// One reviewed pasted line.
pub(crate) struct PasteReviewRow {
    pub line: String,
    pub kind: PasteRowKind,
    pub matched: Option<PasteMatch>,
    pub include: bool,
    /// Why a row is already present ("already in <list>" /
    /// "duplicate line"); display only.
    pub present_note: String,
}

/// Paste-import dialog state: the pasted text, the target choice
/// (same shapes as the CSV dialog), the review rows, and the
/// in-flight matching channel.
#[derive(Default)]
pub(crate) struct PasteImportState {
    pub open: bool,
    pub text: String,
    pub mode_new: bool,
    pub new_name: String,
    pub new_category: Category,
    pub existing_list: Option<u64>,
    pub review: Vec<PasteReviewRow>,
    pub reviewing: bool,
    pub review_rx: Option<Receiver<Vec<Option<PasteMatch>>>>,
    pub review_lines: Vec<String>,
    pub note: String,
    pub message: String,
}

fn review_row(line: &str, matched: Option<PasteMatch>) -> PasteReviewRow {
    PasteReviewRow {
        line: line.to_string(),
        kind: if matched.is_some() {
            PasteRowKind::Matched
        } else {
            PasteRowKind::Manual
        },
        include: true,
        matched,
        present_note: String::new(),
    }
}

/// Build a match from a provider suggestion, mirroring what picking
/// the suggestion in the add flow would store: the suggestion's own
/// fields, its year filed under the category's year key.
fn suggestion_match(category: Category, sug: crate::providers::Suggestion) -> PasteMatch {
    let mut fields = sug.fields.clone();
    if let (Some(key), Some(year)) = (category.year_key(), &sug.year) {
        fields
            .entry(key.to_string())
            .or_insert_with(|| year.clone());
    }
    let detail = match (&sug.year, sug.detail.is_empty()) {
        (Some(y), false) => format!("{y} · {}", sug.detail),
        (Some(y), true) => y.clone(),
        (None, _) => sug.detail.clone(),
    };
    PasteMatch {
        title: sug.title.clone(),
        detail,
        fields,
        provider_id: sug.provider_id.clone(),
        source: sug.source.to_string(),
    }
}

/// Build a match from a drinks-database entry — the same fields the
/// add flow's `suggestion_from_drink` builds.
fn drink_match(e: crate::drinks::DrinkEntry) -> PasteMatch {
    let mut fields = BTreeMap::new();
    if !e.brand.is_empty() {
        fields.insert("brand".to_string(), e.brand.clone());
    }
    if !e.flavor.is_empty() {
        fields.insert("flavor".to_string(), e.flavor.clone());
    }
    if let Some(label) = e.caffeine_label() {
        fields.insert("caffeine".to_string(), label);
    }
    PasteMatch {
        title: e.name.clone(),
        detail: e.detail_line(),
        fields,
        provider_id: format!("{}:{}", e.brand, e.name),
        source: "drinks-db".to_string(),
    }
}

/// The probe item a review row would add, for `contains_work`
/// already-present checks (provider identity when matched, bare
/// title otherwise).
fn row_probe(row: &PasteReviewRow) -> Item {
    match &row.matched {
        Some(m) => {
            let mut item = Item::new(0, m.title.clone());
            item.provider_id = Some(m.provider_id.clone());
            item.source = m.source.clone();
            item
        }
        None => Item::new(0, row.line.clone()),
    }
}

impl FaveApp {
    // -- library export (#6) ----------------------------------------------

    pub(crate) fn open_library_export(&mut self) {
        self.library_export_ui = LibraryExportState {
            open: true,
            ..LibraryExportState::default()
        };
    }

    pub(crate) fn library_export_dialog(&mut self, ctx: &egui::Context) {
        if !self.library_export_ui.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Export library")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Export every list: CSV for spreadsheets (one row per \
                         entry), or JSON in the app's own format — the same \
                         shape the data folder stores.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.library_export_ui.format_json, false, "CSV");
                    ui.radio_value(&mut self.library_export_ui.format_json, true, "JSON");
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Export…").clicked() {
                        let json_fmt = self.library_export_ui.format_json;
                        let (ext, label) = if json_fmt {
                            ("json", "JSON")
                        } else {
                            ("csv", "CSV")
                        };
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("favelists-library.{ext}"))
                            .add_filter(label, &[ext])
                            .save_file()
                        {
                            match self.dispatch(
                                "export.library",
                                serde_json::json!({
                                    "format": if json_fmt { "json" } else { "csv" },
                                    "path": path.to_string_lossy().into_owned(),
                                }),
                            ) {
                                Ok(r) => {
                                    self.status = format!(
                                        "Exported library to {} ({} lists, {} items).",
                                        path.display(),
                                        r["lists"],
                                        r["items"]
                                    );
                                    self.library_export_ui.open = false;
                                }
                                Err(e) => {
                                    self.library_export_ui.message = format!("Export failed: {e}");
                                }
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.library_export_ui.open = false;
                    }
                });
                if !self.library_export_ui.message.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&self.library_export_ui.message).small());
                }
            });
        if !open {
            self.library_export_ui.open = false;
        }
    }

    // -- paste-a-list import (#7) -------------------------------------------

    pub(crate) fn open_paste_import(&mut self) {
        self.paste_ui = PasteImportState {
            open: true,
            mode_new: true,
            existing_list: self.library.lists.first().map(|l| l.id),
            ..PasteImportState::default()
        };
    }

    /// The non-blank pasted lines, trimmed — the review's input and
    /// the cap the command enforces (500) is checked against these.
    fn paste_lines(&self) -> Vec<String> {
        self.paste_ui
            .text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn paste_target_category(&self) -> Option<Category> {
        if self.paste_ui.mode_new {
            Some(self.paste_ui.new_category)
        } else {
            self.paste_ui
                .existing_list
                .and_then(|id| self.library.list(id))
                .map(|l| l.category)
        }
    }

    /// Run the match review for the current lines + target: drinks
    /// categories match locally at once; other categories search
    /// their provider on a background thread — unless the provider
    /// needs keys the user hasn't set, in which case matching is
    /// skipped, every row is Manual, and the dialog says why.
    fn start_paste_review(&mut self) {
        let lines = self.paste_lines();
        if lines.is_empty() {
            self.paste_ui.note = "Paste at least one title first.".to_string();
            return;
        }
        if lines.len() > crate::paste_import::MAX_PASTE_ROWS {
            self.paste_ui.note = format!(
                "That's {} lines — a paste import takes at most {}.",
                lines.len(),
                crate::paste_import::MAX_PASTE_ROWS
            );
            return;
        }
        let Some(category) = self.paste_target_category() else {
            self.paste_ui.note = "Pick a target list first.".to_string();
            return;
        };
        self.paste_ui.note.clear();
        self.paste_ui.message.clear();
        self.paste_ui.review.clear();
        if let Some(kind) = category.drinks_kind() {
            self.paste_ui.review = lines
                .iter()
                .map(|line| {
                    let matched = self
                        .drinks
                        .search(kind, line, 1)
                        .into_iter()
                        .next()
                        .map(drink_match);
                    review_row(line, matched)
                })
                .collect();
            self.mark_paste_present();
            return;
        }
        if let Err(e) = crate::providers::availability(category, &self.settings) {
            self.paste_ui.review = lines.iter().map(|line| review_row(line, None)).collect();
            self.paste_ui.note = format!(
                "No provider matching for {} — {e} Every line below will be \
                 added as a manual entry instead.",
                category.name()
            );
            self.mark_paste_present();
            return;
        }
        let settings = self.settings.clone();
        let (tx, rx) = channel();
        self.paste_ui.review_rx = Some(rx);
        self.paste_ui.reviewing = true;
        self.paste_ui.review_lines = lines.clone();
        std::thread::spawn(move || {
            let results: Vec<Option<PasteMatch>> = lines
                .iter()
                .map(|line| {
                    crate::providers::search(category, line, &settings)
                        .ok()
                        .and_then(|mut sugs| {
                            if sugs.is_empty() {
                                None
                            } else {
                                Some(sugs.remove(0))
                            }
                        })
                        .map(|sug| suggestion_match(category, sug))
                })
                .collect();
            let _ = tx.send(results);
        });
    }

    /// Collect a finished background review, if one has landed.
    fn poll_paste_review(&mut self, ctx: &egui::Context) {
        if !self.paste_ui.reviewing {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
        let Some(rx) = &self.paste_ui.review_rx else {
            self.paste_ui.reviewing = false;
            return;
        };
        match rx.try_recv() {
            Ok(results) => {
                let lines = std::mem::take(&mut self.paste_ui.review_lines);
                self.paste_ui.review = lines
                    .iter()
                    .zip(results)
                    .map(|(line, m)| review_row(line, m))
                    .collect();
                self.paste_ui.review_rx = None;
                self.paste_ui.reviewing = false;
                self.mark_paste_present();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.paste_ui.review_rx = None;
                self.paste_ui.reviewing = false;
                self.paste_ui.note =
                    "Matching stopped unexpectedly — reopen the dialog to try again.".to_string();
            }
        }
    }

    /// Mark rows whose work is already in the destination list (or
    /// repeats an earlier line — the import keeps the first) as
    /// AlreadyPresent and uncheck them.
    fn mark_paste_present(&mut self) {
        let target_list = if self.paste_ui.mode_new {
            None
        } else {
            self.paste_ui
                .existing_list
                .and_then(|id| self.library.list(id))
                .cloned()
        };
        let mut earlier: Vec<Item> = Vec::new();
        for row in &mut self.paste_ui.review {
            let probe = row_probe(row);
            if let Some(list) = &target_list {
                if list.contains_work(&probe) {
                    row.kind = PasteRowKind::AlreadyPresent;
                    row.include = false;
                    row.present_note = format!("already in {}", list.name);
                    continue;
                }
            }
            if earlier.iter().any(|i| i.work_key() == probe.work_key()) {
                row.kind = PasteRowKind::AlreadyPresent;
                row.include = false;
                row.present_note = "duplicate line".to_string();
                continue;
            }
            earlier.push(probe);
        }
    }

    pub(crate) fn paste_import_dialog(&mut self, ctx: &egui::Context) {
        if !self.paste_ui.open {
            return;
        }
        self.poll_paste_review(ctx);
        let target_before = (
            self.paste_ui.mode_new,
            self.paste_ui.new_category,
            self.paste_ui.existing_list,
        );
        let mut open = true;
        egui::Window::new("Paste a list")
            .open(&mut open)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Paste one title per line, then review the matches \
                         before anything is added. Importing is merge-only: \
                         works already in the destination list are skipped, \
                         never overwritten.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.paste_ui.text)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY)
                        .hint_text("One title per line…"),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.paste_ui.mode_new, true, "New list");
                    ui.radio_value(&mut self.paste_ui.mode_new, false, "Existing list");
                });
                if self.paste_ui.mode_new {
                    ui.horizontal(|ui| {
                        ui.label("List name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.paste_ui.new_name)
                                .desired_width(200.0),
                        );
                        ui.label("Category:");
                        egui::ComboBox::from_id_salt("paste-category")
                            .selected_text(self.paste_ui.new_category.name())
                            .show_ui(ui, |ui| {
                                for cat in Category::ALL {
                                    ui.selectable_value(
                                        &mut self.paste_ui.new_category,
                                        cat,
                                        cat.name(),
                                    );
                                }
                            });
                    });
                } else {
                    let chosen = self
                        .paste_ui
                        .existing_list
                        .and_then(|id| self.library.list(id));
                    let list_options: Vec<(u64, String)> = self
                        .library
                        .lists
                        .iter()
                        .map(|l| (l.id, format!("{} ({})", l.name, l.category.name())))
                        .collect();
                    egui::ComboBox::from_id_salt("paste-target-list")
                        .selected_text(
                            chosen
                                .map(|l| format!("{} ({})", l.name, l.category.name()))
                                .unwrap_or_else(|| "Pick a list…".to_string()),
                        )
                        .show_ui(ui, |ui| {
                            for (id, label) in &list_options {
                                ui.selectable_value(
                                    &mut self.paste_ui.existing_list,
                                    Some(*id),
                                    label.as_str(),
                                );
                            }
                        });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let lines = self.paste_lines();
                    let can_review = !self.paste_ui.reviewing
                        && !lines.is_empty()
                        && self.paste_target_category().is_some();
                    if ui
                        .add_enabled(can_review, egui::Button::new("Review matches"))
                        .clicked()
                    {
                        self.start_paste_review();
                    }
                    if self.paste_ui.reviewing {
                        ui.spinner();
                        ui.label("Matching…");
                    } else if !lines.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("{} line(s)", lines.len()))
                                .color(COL_MUTED)
                                .small(),
                        );
                    }
                });
                if !self.paste_ui.note.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.paste_ui.note)
                            .color(COL_ACCENT)
                            .small(),
                    );
                }
                if !self.paste_ui.review.is_empty() {
                    ui.add_space(4.0);
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(230.0)
                        .show(ui, |ui| {
                            for row in &mut self.paste_ui.review {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut row.include, "");
                                    let (tag, color) = match row.kind {
                                        PasteRowKind::Matched => ("Matched", None),
                                        PasteRowKind::Manual => ("Manual", Some(COL_MUTED)),
                                        PasteRowKind::AlreadyPresent => {
                                            ("Already present", Some(COL_ACCENT))
                                        }
                                    };
                                    let tag_text = egui::RichText::new(tag).small();
                                    let tag_text = match color {
                                        Some(c) => tag_text.color(c),
                                        None => tag_text,
                                    };
                                    ui.label(tag_text);
                                    match &row.matched {
                                        Some(m) => {
                                            ui.vertical(|ui| {
                                                ui.label(&m.title);
                                                let mut sub = m.detail.clone();
                                                if row.line != m.title {
                                                    sub =
                                                        format!("{sub} — pasted as “{}”", row.line);
                                                }
                                                if row.kind == PasteRowKind::AlreadyPresent {
                                                    sub = format!("{sub} ({})", row.present_note);
                                                }
                                                ui.label(
                                                    egui::RichText::new(sub)
                                                        .color(COL_MUTED)
                                                        .small(),
                                                );
                                            });
                                        }
                                        None => {
                                            ui.vertical(|ui| {
                                                ui.label(&row.line);
                                                let sub = if row.kind
                                                    == PasteRowKind::AlreadyPresent
                                                {
                                                    row.present_note.clone()
                                                } else {
                                                    "no provider match — added as typed".to_string()
                                                };
                                                ui.label(
                                                    egui::RichText::new(sub)
                                                        .color(COL_MUTED)
                                                        .small(),
                                                );
                                            });
                                        }
                                    }
                                });
                            }
                        });
                    let included: Vec<usize> = self
                        .paste_ui
                        .review
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r.include)
                        .map(|(i, _)| i)
                        .collect();
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let target_ok = if self.paste_ui.mode_new {
                            !self.paste_ui.new_name.trim().is_empty()
                        } else {
                            self.paste_ui.existing_list.is_some()
                        };
                        let can_add = !self.paste_ui.reviewing && !included.is_empty() && target_ok;
                        if ui
                            .add_enabled(
                                can_add,
                                egui::Button::new(format!("Add {} item(s)", included.len())),
                            )
                            .clicked()
                        {
                            self.add_reviewed_paste(&included);
                        }
                        if ui.button("Close").clicked() {
                            self.paste_ui.open = false;
                        }
                    });
                } else {
                    ui.add_space(4.0);
                    if ui.button("Close").clicked() {
                        self.paste_ui.open = false;
                    }
                }
                if !self.paste_ui.message.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&self.paste_ui.message).small());
                }
            });
        if !open {
            self.paste_ui.open = false;
        }
        // A target change invalidates the review: matches and
        // already-present marks were computed against the old target.
        let target_after = (
            self.paste_ui.mode_new,
            self.paste_ui.new_category,
            self.paste_ui.existing_list,
        );
        if target_after != target_before && !self.paste_ui.reviewing {
            self.paste_ui.review.clear();
            self.paste_ui.note = "Target changed — review the matches again.".to_string();
        }
    }

    /// Dispatch `import.paste` once for the checked review rows and
    /// report the summary in the house shape.
    fn add_reviewed_paste(&mut self, included: &[usize]) {
        let rows_json: Vec<serde_json::Value> = included
            .iter()
            .filter_map(|&i| self.paste_ui.review.get(i))
            .map(|row| match &row.matched {
                Some(m) => serde_json::json!({
                    "title": m.title,
                    "fields": m.fields,
                    "provider_id": m.provider_id,
                    "source": m.source,
                }),
                None => serde_json::json!({"title": row.line}),
            })
            .collect();
        let target = if self.paste_ui.mode_new {
            serde_json::json!({
                "mode": "new",
                "name": self.paste_ui.new_name.trim(),
                "category": self.paste_ui.new_category.id(),
            })
        } else {
            serde_json::json!({
                "mode": "existing",
                "list_id": self.paste_ui.existing_list,
            })
        };
        match self.dispatch(
            "import.paste",
            serde_json::json!({"rows": rows_json, "target": target}),
        ) {
            Ok(r) => {
                let n = |k: &str| r[k].as_u64().unwrap_or(0);
                let mut msg = format!(
                    "Added {}, skipped {} already present.",
                    n("added"),
                    n("skipped_duplicates")
                );
                if n("skipped_empty") > 0 {
                    msg += &format!(" {} blank row(s) were ignored.", n("skipped_empty"));
                }
                self.status = msg.clone();
                self.paste_ui.message = msg;
                self.paste_ui.text.clear();
                self.paste_ui.review.clear();
                self.repair_selection();
                self.persist();
            }
            Err(e) => {
                self.paste_ui.message = format!("Import failed: {e}");
            }
        }
    }
}
