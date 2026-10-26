//! Smart views + "Pick for me" GUI (v0.3 program #2 + #3 + #1) —
//! another `impl FaveApp` block, split out of `app.rs` for the same
//! reason as `app_bulk.rs` / `app_share.rs`: that file is near the
//! publish-size ceiling.
//!
//! Everything here collects intent and dispatches through the command
//! registry (`library.view` data comes from the same pure
//! [`crate::views`] computation the command serialises; picks dispatch
//! `library.pick`; view share cards dispatch `export.share_card` with
//! a `view` param). Decision logic lives in the library, not here.

use crate::app::{FaveApp, COL_ACCENT, COL_HAIRLINE, COL_MUTED, COL_PANEL, COL_TEXT};
use crate::model::FavList;
use crate::views::{SmartView, ViewEntry};
use eframe::egui;
use std::time::Instant;

/// How long the pick reveal cycles candidate titles before settling
/// on the command's pick (the pick itself is final from the start —
/// the cycling is cosmetic only).
const REVEAL_TOTAL_MS: u128 = 600;
/// How long each cycled title stays up during the reveal.
const REVEAL_STEP_MS: u128 = 75;

/// Smart-view + pick state: which view (if any) fills the main area,
/// and the in-flight pick reveal.
#[derive(Default)]
pub(crate) struct ViewsState {
    pub active: Option<SmartView>,
    pub pick: Option<PickState>,
}

/// One pick, from dispatch to reveal. `entry` is the command's result
/// — final from the start; `candidates` are the scope's titles, cycled
/// briefly for feel before the dialog settles on `entry`.
pub(crate) struct PickState {
    entry: ViewEntry,
    scope_label: String,
    candidates: Vec<String>,
    /// The pick params, kept so "Pick again" replays the same scope.
    params: serde_json::Value,
    started: Instant,
    settled: bool,
}

enum PickAction {
    None,
    Close,
    Open,
    Again,
}

impl FaveApp {
    // -- sidebar section ---------------------------------------------------

    /// The sidebar's Smart views section: the three views as plain
    /// selectable labels (the sidebar's list-row style), plus the
    /// whole-library Pick button.
    pub(crate) fn smart_views_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Smart views").strong());
        ui.add_space(2.0);
        for view in SmartView::ALL {
            let selected = self.views.active == Some(view);
            if ui.selectable_label(selected, view.name()).clicked() {
                self.views.active = Some(view);
                // The view takes the main area: drop the surfaces that
                // would mask it (duplicates view, global search, the
                // missing-cover sweep, the trash view).
                self.dup.open = false;
                self.covers_ui.open = false;
                self.trash_ui.open = false;
                self.global_query.clear();
            }
        }
        ui.add_space(2.0);
        // Missing-cover sweep (v0.3 #9, app_covers.rs): the count is
        // live — the same computation the sweep lists.
        let missing = crate::views::missing_cover_entries(&self.library).len();
        if ui
            .selectable_label(self.covers_ui.open, format!("Missing covers ({missing})"))
            .clicked()
        {
            self.covers_ui.open = true;
            self.views.active = None;
            self.dup.open = false;
            self.trash_ui.open = false;
            self.global_query.clear();
        }
        ui.add_space(2.0);
        // Trash (v0.3 #10, app_trash.rs): the count is the library's
        // trash records — the store purges expired ones at load, and
        // opening the view purges again before showing it.
        let trashed = self.library.trash.len();
        if ui
            .selectable_label(self.trash_ui.open, format!("Trash ({trashed})"))
            .clicked()
        {
            self.open_trash();
            self.views.active = None;
            self.covers_ui.open = false;
            self.dup.open = false;
            self.global_query.clear();
        }
        ui.add_space(2.0);
        if ui.button("🎲 Pick for me").clicked() {
            let candidates: Vec<String> = crate::views::all_entries(&self.library)
                .into_iter()
                .map(|e| e.item.title)
                .collect();
            self.start_pick(
                serde_json::json!({}),
                "your whole library".to_string(),
                candidates,
            );
        }
    }

    // -- the view itself ---------------------------------------------------

    pub(crate) fn smart_view_ui(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.views.active else {
            return;
        };
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.views.active = None;
            return;
        }
        // Computed fresh each frame from the pure views layer — the
        // same entries `library.view` serialises — so edits and undos
        // show up immediately.
        let entries = crate::views::view_items(&self.library, view);
        let mut close = false;
        let mut share = false;
        let mut pick = false;
        ui.horizontal(|ui| {
            ui.heading(view.name());
            ui.label(egui::RichText::new(format!("{} entries", entries.len())).color(COL_MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕ Close").clicked() {
                    close = true;
                }
                if ui.button("🎲 Pick for me").clicked() {
                    pick = true;
                }
                if ui.button("Share card…").clicked() {
                    share = true;
                }
            });
        });
        ui.separator();
        if close {
            self.views.active = None;
            return;
        }
        if share {
            self.open_share_card_for_view(view);
        }
        if pick {
            let candidates = entries.iter().map(|e| e.item.title.clone()).collect();
            self.start_pick(
                serde_json::json!({"view": view.id()}),
                format!("the {} view", view.name()),
                candidates,
            );
        }
        if entries.is_empty() {
            ui.label(egui::RichText::new(empty_view_text(view)).color(COL_MUTED));
            return;
        }
        let mut jump: Option<(u64, u64)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &entries {
                if self.view_entry_row(ui, entry) {
                    jump = Some((entry.list_id, entry.item.id));
                }
            }
        });
        if let Some((list_id, item_id)) = jump {
            self.jump_to_item(list_id, item_id);
        }
    }

    /// One view row, rendered like the search results but richer:
    /// cover thumbnail, title (+ year), list name · category, and the
    /// ★ rating when the item has one. Returns true when clicked.
    fn view_entry_row(&mut self, ui: &mut egui::Ui, entry: &ViewEntry) -> bool {
        let item = &entry.item;
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 58.0), egui::Sense::click());
        let painter = ui.painter_at(rect);
        if resp.hovered() {
            painter.rect_filled(rect, 4.0, COL_PANEL);
        }
        // Cover thumbnail (or an initial tile, like the grid's
        // placeholder treatment).
        let thumb =
            egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 3.0), egui::vec2(38.0, 52.0));
        let tex = item
            .cover
            .as_deref()
            .and_then(|c| self.cover_texture(ui.ctx(), c));
        match tex {
            Some(tex) => {
                painter.image(
                    tex.id(),
                    thumb,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                painter.rect_filled(thumb, 3.0, COL_PANEL);
                painter.rect_stroke(
                    thumb,
                    3.0,
                    egui::Stroke::new(1.0f32, COL_HAIRLINE),
                    egui::StrokeKind::Inside,
                );
                let initial = item
                    .title
                    .trim()
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>())
                    .unwrap_or_else(|| "?".to_string());
                painter.text(
                    thumb.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    egui::FontId::proportional(16.0),
                    COL_MUTED,
                );
            }
        }
        // Title (+ year), then list · category underneath.
        let text_x = thumb.max.x + 10.0;
        let mid_y = rect.center().y;
        let year = entry
            .category
            .year_key()
            .and_then(|k| item.field(k))
            .filter(|y| !y.trim().is_empty());
        let title = match year {
            Some(y) => format!("{} ({})", item.title, y),
            None => item.title.clone(),
        };
        painter.text(
            egui::pos2(text_x, mid_y - 10.0),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(15.0),
            COL_TEXT,
        );
        painter.text(
            egui::pos2(text_x, mid_y + 12.0),
            egui::Align2::LEFT_CENTER,
            format!("{} · {}", entry.list_name, entry.category.name()),
            egui::FontId::proportional(12.0),
            COL_MUTED,
        );
        if let Some(r) = item.rating {
            painter.text(
                egui::pos2(rect.max.x - 8.0, mid_y),
                egui::Align2::RIGHT_CENTER,
                format!("★ {r}"),
                egui::FontId::proportional(14.0),
                COL_ACCENT,
            );
        }
        resp.clicked()
    }

    /// Jump to an item in its list — the global-search jump's steps
    /// (select the list, clear the filters, select the item so the
    /// detail panel opens), plus closing the smart view.
    pub(crate) fn jump_to_item(&mut self, list_id: u64, item_id: u64) {
        self.selected_list = Some(list_id);
        self.bulk_selection.clear();
        self.filter.clear();
        self.global_query.clear();
        self.views.active = None;
        self.select_item(item_id);
    }

    // -- Pick for me --------------------------------------------------------

    /// The list header's Pick button (scope = that list).
    pub(crate) fn pick_from_list(&mut self, list: &FavList) {
        let candidates = list.items.iter().map(|i| i.title.clone()).collect();
        self.start_pick(
            serde_json::json!({"list_id": list.id}),
            format!("“{}”", list.name),
            candidates,
        );
    }

    /// Dispatch `library.pick` for a scope and open the reveal dialog
    /// with the result. An empty scope says so in the status line
    /// instead of opening an empty dialog.
    pub(crate) fn start_pick(
        &mut self,
        params: serde_json::Value,
        scope_label: String,
        candidates: Vec<String>,
    ) {
        match self.dispatch("library.pick", params.clone()) {
            Ok(result) => match crate::views::entry_from_json(&result["entry"]) {
                Some(entry) => {
                    // Nothing to cycle when the scope held one title.
                    let settled = candidates.len() <= 1;
                    self.views.pick = Some(PickState {
                        entry,
                        scope_label,
                        candidates,
                        params,
                        started: Instant::now(),
                        settled,
                    });
                }
                None => {
                    self.status = format!("Nothing to pick from {scope_label} yet.");
                }
            },
            Err(e) => {
                self.status = format!("Pick failed: {e}");
            }
        }
    }

    /// The reveal dialog: cycle the scope's titles for ~0.6 s, then
    /// settle on the command's pick (cover + title + list name), with
    /// Pick again / Open / Close.
    pub(crate) fn pick_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pick) = self.views.pick.take() else {
            return;
        };
        let mut action = PickAction::None;
        let mut open = true;
        egui::Window::new("🎲 Pick for me")
            .open(&mut open)
            .resizable(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                if !pick.settled {
                    let elapsed = pick.started.elapsed().as_millis();
                    if elapsed >= REVEAL_TOTAL_MS || pick.candidates.len() <= 1 {
                        pick.settled = true;
                    } else {
                        let idx = (elapsed / REVEAL_STEP_MS) as usize % pick.candidates.len();
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(format!("Picking from {}…", pick.scope_label))
                                    .color(COL_MUTED)
                                    .small(),
                            );
                            ui.add_space(4.0);
                            ui.heading(pick.candidates[idx].as_str());
                            ui.add_space(12.0);
                        });
                        ui.ctx().request_repaint();
                    }
                }
                if pick.settled {
                    let entry = &pick.entry;
                    ui.vertical_centered(|ui| {
                        if let Some(cover) = &entry.item.cover {
                            if let Some(tex) = self.cover_texture(ui.ctx(), cover) {
                                ui.image((tex.id(), egui::vec2(120.0, 180.0)));
                                ui.add_space(6.0);
                            }
                        }
                        ui.heading(entry.item.title.as_str());
                        let mut meta = format!("from {}", entry.list_name);
                        if let Some(r) = entry.item.rating {
                            meta += &format!(" · ★ {r}");
                        }
                        ui.label(egui::RichText::new(meta).color(COL_MUTED));
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("🎲 Pick again").clicked() {
                            action = PickAction::Again;
                        }
                        if ui.button("Open").clicked() {
                            action = PickAction::Open;
                        }
                        if ui.button("Close").clicked() {
                            action = PickAction::Close;
                        }
                    });
                }
            });
        if !open {
            action = PickAction::Close;
        }
        match action {
            PickAction::None => {
                self.views.pick = Some(pick);
            }
            PickAction::Close => {}
            PickAction::Open => {
                let (list_id, item_id) = (pick.entry.list_id, pick.entry.item.id);
                self.jump_to_item(list_id, item_id);
            }
            PickAction::Again => {
                self.start_pick(
                    pick.params.clone(),
                    pick.scope_label.clone(),
                    pick.candidates.clone(),
                );
            }
        }
    }
}

fn empty_view_text(view: SmartView) -> &'static str {
    match view {
        SmartView::TopRated => {
            "No rated items yet — rate items in their detail panel and they will show up here."
        }
        SmartView::RecentlyAdded => "No items in any list yet.",
        SmartView::NeedsRating => "Everything is rated. Nice.",
    }
}
