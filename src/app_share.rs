//! Share-card export + CSV import dialogs (v0.2 program #8 + #10) — a
//! third `impl FaveApp` block, split out of `app.rs` for the same reason
//! as `app_bulk.rs`: that file is near the publish-size ceiling. Since
//! v0.3 Slice 2 (#3) the share-card dialog also serves smart views:
//! same dialog, `view` param instead of `list_id`.
//!
//! Both dialogs only collect intent and dispatch through the command
//! registry (`export.share_card`, `import.csv`), exactly like the rest
//! of the GUI. The CSV detection readout comes from
//! `csv_import::inspect` — the same parse the command re-runs on
//! import, so what the dialog promises is what the import does.

use crate::app::{FaveApp, COL_ACCENT, COL_MUTED};
use crate::csv_import::CsvInspection;
use crate::model::FavList;
use eframe::egui;
use std::path::PathBuf;

/// Share-card dialog state: which list — or, since v0.3 Slice 2 (#3),
/// which smart view (`view` set instead of a meaningful `list_id`) —
/// and the entry-count choice (5 / 10 / 25, where 25 is labelled "All"
/// — the card's maximum).
#[derive(Default)]
pub(crate) struct ShareCardState {
    pub open: bool,
    pub list_id: u64,
    pub view: Option<crate::views::SmartView>,
    pub count: usize,
    pub message: String,
}

/// CSV import dialog state: the picked file and its detection readout,
/// the target choice, and the last import's summary / error.
#[derive(Default)]
pub(crate) struct CsvImportState {
    pub open: bool,
    pub path: Option<PathBuf>,
    pub inspection: Option<CsvInspection>,
    pub pick_error: String,
    pub mode_new: bool,
    pub new_name: String,
    pub existing_list: Option<u64>,
    pub message: String,
}

impl FaveApp {
    // -- share card (#8) ---------------------------------------------------

    pub(crate) fn open_share_card(&mut self, list: &FavList) {
        self.share_ui = ShareCardState {
            open: true,
            list_id: list.id,
            view: None,
            count: 10,
            message: String::new(),
        };
    }

    /// Open the share-card dialog for a smart view (Slice 2 #3): same
    /// dialog, but the card takes the view's entries in view order
    /// under the view's name.
    pub(crate) fn open_share_card_for_view(&mut self, view: crate::views::SmartView) {
        self.share_ui = ShareCardState {
            open: true,
            list_id: 0,
            view: Some(view),
            count: 10,
            message: String::new(),
        };
    }

    pub(crate) fn share_card_dialog(&mut self, ctx: &egui::Context) {
        if !self.share_ui.open {
            return;
        }
        // Resolve the card's source up front — a list (the original
        // flow) or a smart view — into the three things the dialog
        // renders: its display title, its entry total, and how many of
        // the first `count` entries have no cover.
        let (title, total, missing) = if let Some(view) = self.share_ui.view {
            let entries = crate::views::view_items(&self.library, view);
            let missing = entries
                .iter()
                .take(self.share_ui.count)
                .filter(|e| e.item.cover.is_none())
                .count();
            (view.name().to_string(), entries.len(), missing)
        } else {
            let Some(list) = self.library.list(self.share_ui.list_id).cloned() else {
                self.share_ui.open = false;
                return;
            };
            let missing = list
                .items
                .iter()
                .take(self.share_ui.count)
                .filter(|i| i.cover.is_none())
                .count();
            (list.name.clone(), list.items.len(), missing)
        };
        let chosen = self.share_ui.count.min(total);
        let mut open = true;
        egui::Window::new("Share card")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("A designed PNG of “{}” (1200×630):", title));
                ui.label(
                    egui::RichText::new(
                        "List name, rank numbers, covers, titles and years — \
                         made for sharing, unlike the cover-only image export.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Entries:");
                    ui.radio_value(&mut self.share_ui.count, 5, "Top 5");
                    ui.radio_value(&mut self.share_ui.count, 10, "Top 10");
                    ui.radio_value(&mut self.share_ui.count, 25, "All (up to 25)");
                });
                if missing > 0 {
                    ui.label(
                        egui::RichText::new(format!(
                            "⚠ {missing} of the first {chosen} item(s) have no cover — \
                             they will appear as initial tiles."
                        ))
                        .color(COL_ACCENT)
                        .small(),
                    );
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save PNG…").clicked() {
                        let stem = crate::export_text::safe_file_stem(&title);
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{stem}-share.png"))
                            .add_filter("PNG image", &["png"])
                            .save_file()
                        {
                            let params = if let Some(view) = self.share_ui.view {
                                serde_json::json!({
                                    "view": view.id(),
                                    "path": path.to_string_lossy().into_owned(),
                                    "count": self.share_ui.count,
                                })
                            } else {
                                serde_json::json!({
                                    "list_id": self.share_ui.list_id,
                                    "path": path.to_string_lossy().into_owned(),
                                    "count": self.share_ui.count,
                                })
                            };
                            match self.dispatch("export.share_card", params) {
                                Ok(r) => {
                                    let n = r["entries"].as_u64().unwrap_or(0);
                                    let m = r["missing_indices"]
                                        .as_array()
                                        .map(|a| a.len())
                                        .unwrap_or(0);
                                    self.status = if m > 0 {
                                        format!(
                                            "Saved share card {} ({n} entries, {m} without covers).",
                                            path.display()
                                        )
                                    } else {
                                        format!(
                                            "Saved share card {} ({n} entries).",
                                            path.display()
                                        )
                                    };
                                    self.share_ui.open = false;
                                }
                                Err(e) => {
                                    self.share_ui.message = format!("Share card failed: {e}");
                                }
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.share_ui.open = false;
                    }
                });
                if !self.share_ui.message.is_empty() {
                    ui.label(egui::RichText::new(&self.share_ui.message).small());
                }
            });
        if !open {
            self.share_ui.open = false;
        }
    }

    // -- CSV import (#10) ----------------------------------------------------

    pub(crate) fn open_csv_import(&mut self) {
        self.csv_ui = CsvImportState {
            open: true,
            mode_new: true,
            ..CsvImportState::default()
        };
    }

    /// Read + inspect a picked CSV file, storing the detection readout
    /// (or the error) in the dialog state.
    fn pick_csv_file(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(text) => match crate::csv_import::inspect(&text) {
                Ok(insp) => {
                    self.csv_ui.new_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Imported")
                        .to_string();
                    self.csv_ui.existing_list = self.library.lists.first().map(|l| l.id);
                    self.csv_ui.mode_new = true;
                    self.csv_ui.inspection = Some(insp);
                    self.csv_ui.path = Some(path);
                    self.csv_ui.pick_error.clear();
                    self.csv_ui.message.clear();
                }
                Err(e) => {
                    self.csv_ui.pick_error = e;
                    self.csv_ui.inspection = None;
                    self.csv_ui.path = None;
                }
            },
            Err(e) => {
                self.csv_ui.pick_error = format!("Could not read {}: {e}", path.display());
                self.csv_ui.inspection = None;
                self.csv_ui.path = None;
            }
        }
    }

    pub(crate) fn csv_import_dialog(&mut self, ctx: &egui::Context) {
        if !self.csv_ui.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Import CSV")
            .open(&mut open)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Import a diary / ratings / library CSV export. Importing is \
                         merge-only: works already in the destination list are \
                         skipped, never overwritten.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                ui.add_space(4.0);
                if ui.button("Choose CSV file…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .pick_file()
                    {
                        self.pick_csv_file(path);
                    }
                }
                if !self.csv_ui.pick_error.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.csv_ui.pick_error)
                            .color(COL_ACCENT)
                            .small(),
                    );
                }
                if let (Some(path), Some(insp)) =
                    (self.csv_ui.path.clone(), self.csv_ui.inspection.clone())
                {
                    ui.add_space(4.0);
                    ui.label(format!("File: {}", path.display()));
                    ui.label(format!("Detected: {}", insp.format.label()));
                    ui.label(format!("{} entries with titles", insp.data_rows));
                    match insp.format.category() {
                        Some(cat) => {
                            ui.label(format!("Category: {}", cat.name()));
                        }
                        None => {
                            let parts: Vec<String> = insp
                                .categories
                                .iter()
                                .map(|(c, n)| format!("{}: {n}", c.name()))
                                .collect();
                            if !parts.is_empty() {
                                ui.label(format!("Splits into: {}", parts.join(" · ")));
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.csv_ui.mode_new, true, "New list");
                        ui.radio_value(&mut self.csv_ui.mode_new, false, "Existing list");
                    });
                    let mut can_import;
                    if self.csv_ui.mode_new {
                        ui.horizontal(|ui| {
                            ui.label("List name:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.csv_ui.new_name)
                                    .desired_width(240.0),
                            );
                        });
                        match insp.format.category() {
                            Some(cat) => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Creates one {} list with that name.",
                                        cat.name()
                                    ))
                                    .color(COL_MUTED)
                                    .small(),
                                );
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new(
                                        "Creates one list per category found, named \
                                         “<name> — <Category>”.",
                                    )
                                    .color(COL_MUTED)
                                    .small(),
                                );
                            }
                        }
                        can_import = !self.csv_ui.new_name.trim().is_empty();
                    } else {
                        let chosen = self
                            .csv_ui
                            .existing_list
                            .and_then(|id| self.library.list(id));
                        // Snapshot the options so the ComboBox closure
                        // never borrows the library while the target
                        // selection is borrowed mutably.
                        let list_options: Vec<(u64, String)> = self
                            .library
                            .lists
                            .iter()
                            .map(|l| (l.id, format!("{} ({})", l.name, l.category.name())))
                            .collect();
                        egui::ComboBox::from_id_salt("csv-target-list")
                            .selected_text(
                                chosen
                                    .map(|l| format!("{} ({})", l.name, l.category.name()))
                                    .unwrap_or_else(|| "Pick a list…".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for (id, label) in &list_options {
                                    ui.selectable_value(
                                        &mut self.csv_ui.existing_list,
                                        Some(*id),
                                        label.as_str(),
                                    );
                                }
                            });
                        can_import = chosen.is_some();
                        if let (Some(fc), Some(l)) = (insp.format.category(), chosen) {
                            if l.category != fc {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "This file is a {} export — pick a {} list, \
                                         or use New list.",
                                        fc.name(),
                                        fc.name()
                                    ))
                                    .color(COL_ACCENT)
                                    .small(),
                                );
                                can_import = false;
                            }
                        }
                    }
                    ui.add_space(6.0);
                    if ui
                        .add_enabled(can_import, egui::Button::new("Import"))
                        .clicked()
                    {
                        let target = if self.csv_ui.mode_new {
                            let mut t = serde_json::json!({
                                "mode": "new",
                                "name": self.csv_ui.new_name.trim(),
                            });
                            if let Some(cat) = insp.format.category() {
                                t["category"] = serde_json::json!(cat.id());
                            }
                            t
                        } else {
                            serde_json::json!({
                                "mode": "existing",
                                "list_id": self.csv_ui.existing_list,
                            })
                        };
                        match self.dispatch(
                            "import.csv",
                            serde_json::json!({
                                "path": path.to_string_lossy().into_owned(),
                                "target": target,
                            }),
                        ) {
                            Ok(r) => {
                                let n = |k: &str| r[k].as_u64().unwrap_or(0);
                                let mut msg = format!(
                                    "Added {}, skipped {} already present.",
                                    n("added"),
                                    n("skipped_duplicates")
                                );
                                if n("skipped_wrong_category") > 0 {
                                    msg += &format!(
                                        " {} row(s) belonged to another category and were skipped.",
                                        n("skipped_wrong_category")
                                    );
                                }
                                if n("invalid") > 0 {
                                    msg += &format!(
                                        " {} row(s) had no title and were ignored.",
                                        n("invalid")
                                    );
                                }
                                self.status = msg.clone();
                                self.csv_ui.message = msg;
                                self.csv_ui.path = None;
                                self.csv_ui.inspection = None;
                                self.repair_selection();
                                self.persist();
                            }
                            Err(e) => {
                                self.csv_ui.message = format!("Import failed: {e}");
                            }
                        }
                    }
                }
                if !self.csv_ui.message.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&self.csv_ui.message).small());
                }
                ui.add_space(4.0);
                if ui.button("Close").clicked() {
                    self.csv_ui.open = false;
                }
            });
        if !open {
            self.csv_ui.open = false;
        }
    }
}
