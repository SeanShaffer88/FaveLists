//! Drinks-depth GUI pieces (v0.3 Slice 4) — another `impl FaveApp`
//! block, split out of `app.rs` for the usual reason (that file sits
//! near the publish-size ceiling):
//!
//! * the barcode-add dialog (#11): the lookup is the `barcode`
//!   provider-family seam (background thread, like provider search);
//!   adding the found product dispatches the ordinary `item.add`;
//! * the drinks list-header extras: the "Tried X of Y" progress line
//!   (#12, counting the pure `drinks::tried_progress`) and the
//!   "Caffeine ≥" filter field (#13, applied by the grid predicate
//!   here; the registry's `list.query` takes the same filter as
//!   `caffeine_min`);
//! * the detail panel's one-click "Tried it" toggle for drink items
//!   (#12), dispatching `item.edit` with the #5 status model.

use crate::app::{FaveApp, COL_ACCENT, COL_MUTED};
use crate::barcode::{self, BarcodeLookup, BarcodeProduct};
use crate::model::{Category, FavList, Item, ItemStatus};
use eframe::egui;
use std::sync::mpsc::{channel, Receiver};

// ---------------------------------------------------------------------------
// Barcode add (#11)
// ---------------------------------------------------------------------------

/// Barcode-dialog state: the code being entered, the in-flight lookup,
/// and the product it found.
#[derive(Default)]
pub(crate) struct BarcodeState {
    pub open: bool,
    pub list_id: u64,
    pub code: String,
    pub looking: bool,
    pub rx: Option<Receiver<Result<BarcodeLookup, String>>>,
    pub found: Option<BarcodeProduct>,
    pub message: String,
}

impl FaveApp {
    /// Open the barcode dialog for a drinks list (fresh state each time).
    pub(crate) fn open_barcode(&mut self, list_id: u64) {
        self.barcode_ui = BarcodeState {
            open: true,
            list_id,
            ..Default::default()
        };
    }

    pub(crate) fn barcode_dialog(&mut self, ctx: &egui::Context) {
        if !self.barcode_ui.open {
            return;
        }
        // Poll the lookup worker.
        if let Some(rx) = &self.barcode_ui.rx {
            if let Ok(result) = rx.try_recv() {
                self.barcode_ui.rx = None;
                self.barcode_ui.looking = false;
                match result {
                    Ok(BarcodeLookup::Found(product)) => {
                        self.barcode_ui.message.clear();
                        self.barcode_ui.found = Some(product);
                    }
                    Ok(BarcodeLookup::NotFound) => {
                        self.barcode_ui.found = None;
                        self.barcode_ui.message = "No product found for that barcode.".to_string();
                    }
                    Err(e) => {
                        self.barcode_ui.found = None;
                        self.barcode_ui.message = e;
                    }
                }
            }
        }
        if self.barcode_ui.looking {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        let mut open = true;
        let mut lookup = false;
        let mut add = false;
        egui::Window::new("Add by barcode")
            .open(&mut open)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.label("Type or scan the barcode (UPC / EAN):");
                let resp = ui.text_edit_singleline(&mut self.barcode_ui.code);
                if resp.changed() {
                    // A new code invalidates the previous result.
                    self.barcode_ui.found = None;
                    self.barcode_ui.message.clear();
                }
                let code = self.barcode_ui.code.trim().to_string();
                let valid = barcode::valid_code(&code);
                if !code.is_empty() && !valid {
                    ui.label(
                        egui::RichText::new("Barcodes are 8–14 digits.")
                            .color(COL_ACCENT)
                            .small(),
                    );
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            valid && !self.barcode_ui.looking,
                            egui::Button::new("Look up"),
                        )
                        .clicked()
                    {
                        lookup = true;
                    }
                    if self.barcode_ui.looking {
                        ui.spinner();
                        ui.label("Looking up…");
                    }
                });
                if let Some(product) = &self.barcode_ui.found {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(egui::RichText::new(&product.title).strong());
                    let mut detail = Vec::new();
                    if !product.brand.is_empty() {
                        detail.push(product.brand.clone());
                    }
                    if !product.flavor.is_empty() {
                        detail.push(product.flavor.clone());
                    }
                    if let Some(label) = &product.caffeine_label {
                        detail.push(format!("Caffeine: {label}"));
                    }
                    if !detail.is_empty() {
                        ui.label(
                            egui::RichText::new(detail.join(" · "))
                                .color(COL_MUTED)
                                .small(),
                        );
                    }
                    ui.add_space(4.0);
                    if ui.button("Add to list").clicked() {
                        add = true;
                    }
                }
                if !self.barcode_ui.message.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(&self.barcode_ui.message)
                            .color(COL_MUTED)
                            .small(),
                    );
                }
            });
        if lookup {
            let code = self.barcode_ui.code.trim().to_string();
            let (tx, rx) = channel();
            self.barcode_ui.rx = Some(rx);
            self.barcode_ui.looking = true;
            self.barcode_ui.found = None;
            self.barcode_ui.message.clear();
            std::thread::spawn(move || {
                let _ = tx.send(barcode::fetch_product(&code));
            });
        }
        if add {
            if let Some(product) = self.barcode_ui.found.take() {
                let fields = serde_json::to_value(product.add_fields()).unwrap_or_default();
                let result = self.dispatch(
                    "item.add",
                    serde_json::json!({
                        "list_id": self.barcode_ui.list_id,
                        "title": product.title,
                        "fields": fields,
                        "provider_id": product.code,
                        "source": "openfoodfacts",
                    }),
                );
                match result {
                    Ok(r) if r["outcome"] == "duplicate" => {
                        self.barcode_ui.message =
                            format!("{} is already in this list.", product.title);
                    }
                    Ok(_) => {
                        self.status = format!("Added {}.", product.title);
                        self.barcode_ui.message = format!("Added {}.", product.title);
                        // Ready for the next scan.
                        self.barcode_ui.code.clear();
                        self.persist();
                    }
                    Err(e) => {
                        self.barcode_ui.message = format!("Could not add: {e}");
                    }
                }
            }
        }
        if !open {
            self.barcode_ui.open = false;
        }
    }

    // -- drinks list header extras (#12 progress, #13 caffeine filter) --

    /// The drinks-only header row under the tag/status filters: the
    /// "Tried X of Y" progress (#12) and the "Caffeine ≥" filter
    /// field (#13). Renders nothing for other categories.
    pub(crate) fn drinks_filter_ui(&mut self, ui: &mut egui::Ui, list: &FavList) {
        if list.category.drinks_kind().is_none() {
            return;
        }
        ui.horizontal(|ui| {
            let (tried, total) = crate::drinks::tried_progress(list);
            if total > 0 {
                ui.label(
                    egui::RichText::new(format!("Tried {tried} of {total}"))
                        .color(COL_MUTED)
                        .small(),
                );
                ui.separator();
            }
            ui.label("Caffeine ≥");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter_caffeine_min)
                    .desired_width(52.0)
                    .hint_text("any"),
            );
            ui.label(egui::RichText::new("mg/100ml").color(COL_MUTED).small());
        });
    }

    /// The grid's caffeine predicate (#13): an empty or unparseable
    /// field means no filtering; otherwise the item must carry a
    /// caffeine value at or above the minimum — the same rule as the
    /// registry's `list.query` `caffeine_min` filter.
    pub(crate) fn passes_caffeine_filter(&self, item: &Item) -> bool {
        let text = self.filter_caffeine_min.trim();
        if text.is_empty() {
            return true;
        }
        let Ok(min) = text.parse::<f64>() else {
            return true;
        };
        crate::drinks::caffeine_value(item)
            .map(|v| v >= min)
            .unwrap_or(false)
    }

    // -- detail panel: one-click "Tried it" (#12) -------------------------

    /// For drink-category items only: a one-click toggle beside the
    /// status picker (rendered by `item_meta_ui`) — "Tried it ✓"
    /// marks the item `tried`, "✓ Tried" clears it — dispatching
    /// `item.edit`, so the toggle is undoable like every other edit
    /// and the picker's state (`edit_status`) stays in sync.
    pub(crate) fn drink_tried_toggle_ui(
        &mut self,
        ui: &mut egui::Ui,
        list_id: u64,
        category: Category,
        item_id: u64,
        status: Option<ItemStatus>,
    ) {
        if category.drinks_kind().is_none() {
            return;
        }
        let tried = status == Some(ItemStatus::Tried);
        let label = if tried { "✓ Tried" } else { "Tried it ✓" };
        ui.add_space(2.0);
        if ui.add(egui::Button::new(label).selected(tried)).clicked() {
            let new_status = if tried { None } else { Some(ItemStatus::Tried) };
            let status_param = new_status.map(|s| s.id());
            match self.dispatch(
                "item.edit",
                serde_json::json!({
                    "list_id": list_id,
                    "item_id": item_id,
                    "status": status_param,
                }),
            ) {
                Ok(_) => {
                    self.edit_status = new_status;
                    self.status = if tried {
                        "Tried mark removed.".to_string()
                    } else {
                        "Marked as tried.".to_string()
                    };
                    self.persist();
                }
                Err(e) => {
                    self.status = format!("Could not update status: {e}");
                }
            }
        }
    }
}
