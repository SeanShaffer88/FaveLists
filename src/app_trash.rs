//! Trash view (v0.3 program #10) — another `impl FaveApp` block,
//! split out of `app.rs` for the usual reason (that file sits near
//! the publish-size ceiling).
//!
//! The view renders the same rows the `trash.list` command reports
//! (both come from [`crate::trash::entries`], so the view and the
//! registry always agree) and every action dispatches through the
//! registry: `trash.restore`, `trash.remove`, `trash.empty`.
//! Opening the view also runs the 30-day purge; the store path
//! purges at load, so the purge here only matters for a session
//! left open across the boundary.

use crate::app::{FaveApp, COL_ACCENT, COL_MUTED};
use eframe::egui;

/// Trash view state: open flag plus the "Empty trash" two-click
/// confirm arm (first click arms, second click empties).
#[derive(Default)]
pub(crate) struct TrashState {
    pub open: bool,
    pub confirm_empty: bool,
}

enum TrashAction {
    Restore(u64),
    DeleteForever(u64),
    Empty,
}

impl FaveApp {
    /// Open the trash view: run the 30-day purge first (persisting
    /// when it dropped anything) so the view never shows a record
    /// the retention window has already reclaimed.
    pub(crate) fn open_trash(&mut self) {
        let dropped =
            crate::trash::purge_expired(&mut self.library.trash, crate::model::now_unix());
        if dropped > 0 {
            self.persist();
        }
        self.trash_ui.confirm_empty = false;
        self.trash_ui.open = true;
    }

    /// The trash view (central panel; Esc/Close exits, like the
    /// smart views and the cover sweep).
    pub(crate) fn trash_view_ui(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.trash_ui.open = false;
            return;
        }
        let entries = crate::trash::entries(&self.library, crate::model::now_unix());
        let mut close = false;
        let mut action: Option<TrashAction> = None;
        ui.horizontal(|ui| {
            ui.heading("Trash");
            ui.label(
                egui::RichText::new(format!("{} item(s) · kept for 30 days", entries.len()))
                    .color(COL_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕ Close").clicked() {
                    close = true;
                }
                if !entries.is_empty() {
                    if self.trash_ui.confirm_empty {
                        if ui
                            .button(egui::RichText::new("Click again to empty").color(COL_ACCENT))
                            .clicked()
                        {
                            action = Some(TrashAction::Empty);
                        }
                    } else if ui.button("Empty trash").clicked() {
                        self.trash_ui.confirm_empty = true;
                    }
                }
            });
        });
        ui.separator();
        if close {
            self.trash_ui.open = false;
            return;
        }
        if entries.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Nothing here. Items you remove from a list wait here for 30 days \
                     before they are gone for good.",
                )
                .color(COL_MUTED),
            );
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &entries {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&entry.title).strong());
                        let days = if entry.days_left == 1 {
                            "1 day left".to_string()
                        } else {
                            format!("{} days left", entry.days_left)
                        };
                        ui.label(
                            egui::RichText::new(format!(
                                "from {} · deleted {} · {days}",
                                entry.from_list_name,
                                crate::model::iso_date(entry.deleted_at),
                            ))
                            .color(COL_MUTED)
                            .small(),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Delete forever").clicked() {
                            action = Some(TrashAction::DeleteForever(entry.item_id));
                        }
                        if ui.button("Restore").clicked() {
                            action = Some(TrashAction::Restore(entry.item_id));
                        }
                    });
                });
                ui.separator();
            }
        });
        match action {
            Some(TrashAction::Restore(item_id)) => self.restore_trashed(item_id),
            Some(TrashAction::DeleteForever(item_id)) => {
                match self.dispatch("trash.remove", serde_json::json!({"item_id": item_id})) {
                    Ok(_) => {
                        self.persist();
                        self.status = "Deleted forever.".to_string();
                    }
                    Err(e) => {
                        self.status = format!("Could not delete: {e}");
                    }
                }
            }
            Some(TrashAction::Empty) => {
                self.trash_ui.confirm_empty = false;
                match self.dispatch("trash.empty", serde_json::json!({})) {
                    Ok(r) => {
                        let n = r["removed"].as_u64().unwrap_or(0);
                        self.persist();
                        self.status = format!("Trash emptied ({n} item(s) deleted forever).");
                    }
                    Err(e) => {
                        self.status = format!("Could not empty the trash: {e}");
                    }
                }
            }
            None => {}
        }
    }

    /// Restore one trashed item to its original list (the command's
    /// default target) and say where it landed.
    fn restore_trashed(&mut self, item_id: u64) {
        let title = self
            .library
            .trash
            .iter()
            .find(|t| t.item.id == item_id)
            .map(|t| t.item.title.clone())
            .unwrap_or_default();
        match self.dispatch("trash.restore", serde_json::json!({"item_id": item_id})) {
            Ok(r) => {
                let list_id = r["restored_to"].as_u64().unwrap_or(0);
                let name = self
                    .library
                    .list(list_id)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                self.persist();
                self.status = format!("Restored “{title}” to {name}.");
            }
            Err(e) => {
                self.status = format!("Could not restore “{title}”: {e}");
            }
        }
    }
}
