//! Bulk actions, drag-to-list, and the duplicates view (v0.2 program
//! #9 + #3) — a second `impl FaveApp` block, split out of `app.rs` to
//! keep that file manageable.
//!
//! Everything here collects intent and dispatches through the command
//! registry (`items.bulk_*`, `library.find_duplicates`,
//! `library.merge_duplicates`), exactly like the rest of the GUI: each
//! bulk action is ONE dispatch, so it persists and undoes as a single
//! step. Decision logic (matching, adoption, skip rules) lives in the
//! library (`duplicates.rs`, `commands.rs`), not here.

use crate::app::{FaveApp, COL_ACCENT, COL_HAIRLINE, COL_MUTED, COL_PANEL};
use crate::duplicates::{Confidence, DupGroup};
use crate::model::FavList;
use eframe::egui;

/// State of the duplicates view: the groups from the last
/// `library.find_duplicates` dispatch and the user's keep choice per
/// group (a `(list_id, item_id)` copy reference; defaults to each
/// group's first copy on refresh).
#[derive(Default)]
pub(crate) struct DuplicatesState {
    pub open: bool,
    pub groups: Vec<DupGroup>,
    pub keep: Vec<Option<(u64, u64)>>,
}

impl FaveApp {
    // -- bulk selection (#9) ---------------------------------------------

    /// Toggle one item in/out of the bulk selection.
    pub(crate) fn toggle_bulk(&mut self, item_id: u64) {
        if let Some(pos) = self.bulk_selection.iter().position(|id| *id == item_id) {
            self.bulk_selection.remove(pos);
        } else {
            self.bulk_selection.push(item_id);
        }
    }

    /// The slim action bar shown above the grid while a bulk selection is
    /// active: Move to… / Copy to… (list pickers), Remove, Clear. Every
    /// action dispatches one bulk command, then clears the selection.
    pub(crate) fn bulk_bar_ui(&mut self, ui: &mut egui::Ui, list: &FavList) {
        if self.bulk_selection.is_empty() {
            return;
        }
        let count = self.bulk_selection.len();
        let others: Vec<(u64, String)> = self
            .library
            .lists
            .iter()
            .filter(|l| l.id != list.id)
            .map(|l| (l.id, l.name.clone()))
            .collect();
        let mut action: Option<(&'static str, Option<u64>)> = None;
        egui::Frame::default()
            .fill(COL_PANEL)
            .stroke(egui::Stroke::new(1.0f32, COL_HAIRLINE))
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{count} selected")).strong());
                    ui.menu_button("Move to…", |ui| {
                        if others.is_empty() {
                            ui.label(
                                egui::RichText::new("No other lists")
                                    .color(COL_MUTED)
                                    .small(),
                            );
                        }
                        for (id, name) in &others {
                            if ui.button(name).clicked() {
                                action = Some(("items.bulk_move", Some(*id)));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Copy to…", |ui| {
                        if others.is_empty() {
                            ui.label(
                                egui::RichText::new("No other lists")
                                    .color(COL_MUTED)
                                    .small(),
                            );
                        }
                        for (id, name) in &others {
                            if ui.button(name).clicked() {
                                action = Some(("items.bulk_copy", Some(*id)));
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .button(egui::RichText::new("Remove").color(COL_ACCENT))
                        .clicked()
                    {
                        action = Some(("items.bulk_remove", None));
                    }
                    if ui.button("Clear").clicked() {
                        self.bulk_selection.clear();
                    }
                });
            });
        if let Some((command, target)) = action {
            self.apply_bulk(list.id, command, target);
        }
    }

    /// Dispatch one bulk command for the current selection against
    /// `list_id`, report the outcome in the status line, persist, and
    /// clear the selection on success.
    fn apply_bulk(&mut self, list_id: u64, command: &str, target: Option<u64>) {
        let ids: Vec<u64> = self.bulk_selection.clone();
        let params = match command {
            "items.bulk_remove" => serde_json::json!({
                "list_id": list_id,
                "item_ids": ids,
            }),
            _ => serde_json::json!({
                "from_list_id": list_id,
                "to_list_id": target,
                "item_ids": ids,
            }),
        };
        let target_name = target.and_then(|id| self.library.list(id).map(|l| l.name.clone()));
        match self.dispatch(command, params) {
            Ok(result) => {
                self.status = match command {
                    "items.bulk_remove" => {
                        let n = result["removed"].as_array().map(|a| a.len()).unwrap_or(0);
                        format!("Removed {n} item(s).")
                    }
                    "items.bulk_move" => {
                        let moved = result["moved"].as_array().map(|a| a.len()).unwrap_or(0);
                        let skipped = result["skipped"].as_array().map(|a| a.len()).unwrap_or(0);
                        let name = target_name.as_deref().unwrap_or("the other list");
                        if moved == 0 && skipped > 0 {
                            "Already in that list — skipped.".to_string()
                        } else if skipped > 0 {
                            format!("Moved {moved} item(s) to {name}; {skipped} already there — skipped.")
                        } else {
                            format!("Moved {moved} item(s) to {name}.")
                        }
                    }
                    _ => {
                        let copied = result["copied"].as_array().map(|a| a.len()).unwrap_or(0);
                        let skipped = result["skipped"].as_array().map(|a| a.len()).unwrap_or(0);
                        let name = target_name.as_deref().unwrap_or("the other list");
                        if copied == 0 && skipped > 0 {
                            "Already in that list — skipped.".to_string()
                        } else if skipped > 0 {
                            format!("Copied {copied} item(s) to {name}; {skipped} already there — skipped.")
                        } else {
                            format!("Copied {copied} item(s) to {name}.")
                        }
                    }
                };
                self.bulk_selection.clear();
                self.repair_selection();
            }
            Err(e) => {
                self.status = format!("Bulk action failed: {e}");
            }
        }
        self.persist();
    }

    // -- drag handling (#9): grid reorder + drag-to-list ------------------

    /// Pointer-release handling for a grid drag, called from `grid_ui`
    /// with this frame's tile rects. A release over a sidebar list row
    /// moves the dragged item to that list (via `items.bulk_move` with a
    /// single id, so the duplicate-skip rule and one-undo-step semantics
    /// come from the registry); a release over another tile reorders as
    /// before. Dropping onto the item's own list row is a no-op.
    pub(crate) fn grid_drop(
        &mut self,
        ui: &mut egui::Ui,
        list: &FavList,
        tile_rects: &[(usize, egui::Rect)],
    ) {
        if !ui.ctx().input(|i| i.pointer.any_released()) {
            return;
        }
        let Some(from) = self.drag_source.take() else {
            return;
        };
        let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos()) else {
            return;
        };
        // Sidebar drop first: the rows belong to other lists.
        if let Some((target_id, _)) = self
            .sidebar_list_rects
            .iter()
            .find(|(_, r)| r.contains(pos))
            .map(|(id, r)| (*id, *r))
        {
            if target_id != list.id {
                if let Some(item) = list.items.get(from) {
                    let (item_id, title) = (item.id, item.title.clone());
                    let target_name = self
                        .library
                        .list(target_id)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    let result = self.dispatch(
                        "items.bulk_move",
                        serde_json::json!({
                            "from_list_id": list.id,
                            "to_list_id": target_id,
                            "item_ids": [item_id],
                        }),
                    );
                    match result {
                        Ok(r) if r["moved"].as_array().map(|a| a.len()).unwrap_or(0) > 0 => {
                            self.status = format!("Moved {title} to {target_name}.");
                            self.bulk_selection.retain(|id| *id != item_id);
                            self.repair_selection();
                        }
                        Ok(_) => {
                            self.status = "Already in that list — skipped.".to_string();
                        }
                        Err(e) => {
                            self.status = format!("Move failed: {e}");
                        }
                    }
                    self.persist();
                }
            }
            return;
        }
        // Otherwise the pre-Slice-3 behaviour: drop onto a tile reorders.
        if let Some((to, _)) = tile_rects.iter().find(|(_, r)| r.contains(pos)) {
            if *to != from {
                let _ = self.dispatch(
                    "item.move",
                    serde_json::json!({"list_id": list.id, "from": from, "to": to}),
                );
                self.persist();
            }
        }
    }

    // -- duplicates view (#3) ----------------------------------------------

    /// (Re)run the finder and open the duplicates view. Keep choices
    /// reset to each group's first copy.
    pub(crate) fn open_duplicates(&mut self) {
        self.refresh_duplicates();
        self.dup.open = true;
    }

    fn refresh_duplicates(&mut self) {
        let groups = match self.dispatch("library.find_duplicates", serde_json::json!({})) {
            Ok(result) => crate::duplicates::groups_from_result(&result),
            Err(_) => Vec::new(),
        };
        self.dup.keep = groups
            .iter()
            .map(|g| g.copies.first().map(|c| (c.list_id, c.item_id)))
            .collect();
        self.dup.groups = groups;
    }

    /// The duplicates view: replaces the central panel while open (same
    /// pattern as the global-search results view). Groups are split into
    /// exact and possible sections; each group offers a Keep choice per
    /// copy and a Merge button.
    pub(crate) fn duplicates_ui(&mut self, ui: &mut egui::Ui) {
        let mut close = false;
        ui.horizontal(|ui| {
            ui.heading("Duplicate works");
            ui.label(
                egui::RichText::new(format!("{} groups", self.dup.groups.len())).color(COL_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕ Close (Esc)").clicked() {
                    close = true;
                }
            });
        });
        ui.separator();
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if close {
            self.dup.open = false;
            return;
        }
        if self.dup.groups.is_empty() {
            ui.label(
                egui::RichText::new(
                    "No duplicates found. The same work in two lists — under the same \
                     or a slightly different title — would show up here.",
                )
                .color(COL_MUTED),
            );
            return;
        }
        let mut merge: Option<usize> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (section, confidence) in [
                ("Exact matches — same work, years agree", Confidence::Exact),
                (
                    "Possible matches — same title, years differ",
                    Confidence::Possible,
                ),
            ] {
                let in_section: Vec<usize> = self
                    .dup
                    .groups
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| g.confidence == confidence)
                    .map(|(i, _)| i)
                    .collect();
                if in_section.is_empty() {
                    continue;
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(section).strong());
                for gi in in_section {
                    let group = self.dup.groups[gi].clone();
                    ui.add_space(6.0);
                    egui::Frame::default()
                        .fill(COL_PANEL)
                        .stroke(egui::Stroke::new(1.0f32, COL_HAIRLINE))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            let first = &group.copies[0];
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · {} copies · {}",
                                    first.title,
                                    group.copies.len(),
                                    group.category.name()
                                ))
                                .strong(),
                            );
                            ui.label(egui::RichText::new("Keep:").color(COL_MUTED).small());
                            let mut keep = self
                                .dup
                                .keep
                                .get(gi)
                                .copied()
                                .flatten()
                                .unwrap_or((first.list_id, first.item_id));
                            for copy in &group.copies {
                                let label = match &copy.year {
                                    Some(y) => {
                                        format!("{} ({}) — in {}", copy.title, y, copy.list_name)
                                    }
                                    None => {
                                        format!("{} — in {}", copy.title, copy.list_name)
                                    }
                                };
                                ui.radio_value(&mut keep, (copy.list_id, copy.item_id), label);
                            }
                            self.dup.keep[gi] = Some(keep);
                            ui.add_space(4.0);
                            if ui.button("Merge group").clicked() {
                                merge = Some(gi);
                            }
                        });
                }
            }
        });
        if let Some(gi) = merge {
            self.merge_group(gi);
        }
    }

    /// Dispatch `library.merge_duplicates` for group `gi` with the
    /// chosen keep copy; every other copy is dropped. Reports the
    /// summary in the status line and refreshes the view.
    fn merge_group(&mut self, gi: usize) {
        let Some(group) = self.dup.groups.get(gi).cloned() else {
            return;
        };
        let keep = self.dup.keep.get(gi).copied().flatten().unwrap_or_else(|| {
            let first = &group.copies[0];
            (first.list_id, first.item_id)
        });
        let drop: Vec<serde_json::Value> = group
            .copies
            .iter()
            .filter(|c| (c.list_id, c.item_id) != keep)
            .map(|c| serde_json::json!({"list_id": c.list_id, "item_id": c.item_id}))
            .collect();
        let kept_title = group
            .copies
            .iter()
            .find(|c| (c.list_id, c.item_id) == keep)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        let params = serde_json::json!({
            "keep": {"list_id": keep.0, "item_id": keep.1},
            "drop": drop,
        });
        match self.dispatch("library.merge_duplicates", params) {
            Ok(result) => {
                let removed = result["removed"].as_array().map(|a| a.len()).unwrap_or(0);
                self.status = format!("Merged: kept {kept_title}, removed {removed} copies.");
                self.bulk_selection.clear();
                self.repair_selection();
            }
            Err(e) => {
                self.status = format!("Merge failed: {e}");
            }
        }
        self.persist();
        self.refresh_duplicates();
    }
}
