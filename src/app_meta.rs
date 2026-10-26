//! Tags + status GUI pieces (v0.3 Slice 1, #4 + #5): the detail
//! panel's tags editor and status picker, and the list header's
//! tag/status filters. Split out of `app.rs` (the same pattern as
//! `app_bulk.rs`, `app_share.rs`, and `app_settings.rs`) to keep that
//! file under the publish-size ceiling. Saves route through the
//! registry (`item.edit`) from the detail panel, so tags and status
//! are undoable like rating and note.

use crate::app::FaveApp;
use crate::model::{Category, Item, ItemStatus};
use eframe::egui;

impl FaveApp {
    /// Detail-panel section: the tags editor (a single line showing the
    /// tags comma-joined, applied by the panel's Save button through
    /// the same `item.edit` dispatch as title/fields/rating/note) and
    /// the status picker with per-category wording.
    pub(crate) fn item_meta_ui(&mut self, ui: &mut egui::Ui, category: Category) {
        ui.label("Tags (comma-separated)");
        ui.text_edit_singleline(&mut self.edit_tags);
        ui.label("Status");
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.edit_status.is_none(), "No status")
                .clicked()
            {
                self.edit_status = None;
            }
            // The picker's options for this category — plus the stored
            // status itself when the category does not offer it (drinks
            // never offer Owned, but a stored Owned must stay visible
            // and clearable, not silently hidden).
            let mut options: Vec<ItemStatus> = ItemStatus::options_for(category).to_vec();
            if let Some(current) = self.edit_status {
                if !options.contains(&current) {
                    options.push(current);
                }
            }
            for status in options {
                if ui
                    .selectable_label(self.edit_status == Some(status), status.label_for(category))
                    .clicked()
                {
                    self.edit_status = Some(status);
                }
            }
        });
    }

    /// List header: tag + status filters, applied on top of the title
    /// filter by the grid (`passes_meta_filters`). Tag choices come
    /// from the library-wide tag counts — the same data the
    /// `library.tags` command returns.
    pub(crate) fn meta_filter_ui(&mut self, ui: &mut egui::Ui, category: Category) {
        ui.horizontal(|ui| {
            ui.label("Tag:");
            let tag_text = self.filter_tag.clone().unwrap_or_else(|| "All".to_string());
            egui::ComboBox::from_id_salt("tag-filter")
                .selected_text(tag_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter_tag, None, "All");
                    for (tag, count) in self.library.tag_counts() {
                        ui.selectable_value(
                            &mut self.filter_tag,
                            Some(tag.clone()),
                            format!("{tag} ({count})"),
                        );
                    }
                });
            ui.label("Status:");
            let status_text = match self.filter_status {
                Some(s) => s.label_for(category).to_string(),
                None => "All".to_string(),
            };
            egui::ComboBox::from_id_salt("status-filter")
                .selected_text(status_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter_status, None, "All");
                    for status in ItemStatus::options_for(category) {
                        ui.selectable_value(
                            &mut self.filter_status,
                            Some(*status),
                            status.label_for(category),
                        );
                    }
                });
        });
    }

    /// The grid's tag/status predicate (the title predicate stays in
    /// `grid_ui`, exactly as before this slice).
    pub(crate) fn passes_meta_filters(&self, item: &Item) -> bool {
        if let Some(tag) = &self.filter_tag {
            let tag_lower = tag.to_lowercase();
            if !item.tags.iter().any(|t| t.to_lowercase() == tag_lower) {
                return false;
            }
        }
        if let Some(status) = self.filter_status {
            if item.status != Some(status) {
                return false;
            }
        }
        true
    }

    /// The detail panel's tags text → the `tags` array for `item.edit`:
    /// split on commas, trim, drop empties. The registry applies the
    /// full normalisation (dedupe, sort, limits) and rejects bad input.
    pub(crate) fn edit_tags_param(&self) -> Vec<String> {
        self.edit_tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}
