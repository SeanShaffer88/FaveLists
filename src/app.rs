//! FaveLists GUI (egui). Renders state and collects intent; all decision
//! logic lives in the library modules. Network work (provider search,
//! detail/cover fetch, drinks DB update) runs on background threads and
//! reports back over channels polled in `update`.

use crate::backup::{BackupScope, ImportReport};
use crate::drinks::{DrinkKind, DrinksDb};
use crate::layout::{self, Arrangement};
use crate::model::{Category, FavList, Library, Settings, SortKind};
use crate::providers::{self, Suggestion};
use crate::render::{FitMode, ImageFormat};
use crate::store::Store;
use eframe::egui;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Colours (family palette)
// ---------------------------------------------------------------------------
pub const COL_BG: egui::Color32 = egui::Color32::from_rgb(0x10, 0x10, 0x10);
pub const COL_PANEL: egui::Color32 = egui::Color32::from_rgb(0x16, 0x16, 0x16);
pub const COL_HAIRLINE: egui::Color32 = egui::Color32::from_rgb(0x24, 0x24, 0x24);
pub const COL_ACCENT: egui::Color32 = egui::Color32::from_rgb(0xE5, 0x48, 0x4D);
pub const COL_DEEP_RED: egui::Color32 = egui::Color32::from_rgb(0x6B, 0x07, 0x00);
pub const COL_TEXT: egui::Color32 = egui::Color32::from_rgb(0xF5, 0xF5, 0xF5);
pub const COL_MUTED: egui::Color32 = egui::Color32::from_rgb(0xA8, 0xA8, 0xA8);

pub fn install_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = COL_BG;
    visuals.window_fill = COL_PANEL;
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0A, 0x0A, 0x0A);
    visuals.faint_bg_color = egui::Color32::from_rgb(0x1B, 0x1B, 0x1B);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0f32, COL_HAIRLINE);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E);
    visuals.widgets.hovered.bg_fill = COL_DEEP_RED;
    visuals.widgets.active.bg_fill = COL_DEEP_RED;
    visuals.selection.bg_fill = COL_DEEP_RED;
    visuals.selection.stroke = egui::Stroke::new(1.0f32, COL_ACCENT);
    visuals.hyperlink_color = COL_ACCENT;
    ctx.set_visuals(visuals);
}

// ---------------------------------------------------------------------------
// Background messages
// ---------------------------------------------------------------------------

struct SuggestionView {
    suggestion: Suggestion,
    thumb: Option<egui::ColorImage>,
}

enum BgMsg {
    Search {
        list_id: u64,
        query: String,
        result: Result<Vec<SuggestionView>, String>,
    },
    AddReady {
        list_id: u64,
        title: String,
        item_fields: BTreeMap<String, String>,
        provider_id: Option<String>,
        source: String,
        cover_bytes: Option<Vec<u8>>,
    },
    DrinksUpdate(Result<String, String>),
}

struct PendingAdd {
    title: String,
    fields: BTreeMap<String, String>,
    provider_id: Option<String>,
    source: String,
    cover_bytes: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Dialog state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct NewListState {
    open: bool,
    name: String,
    category: Category,
    ranked: bool,
}

#[derive(Default)]
struct ManualState {
    open: bool,
    title: String,
    fields: BTreeMap<String, String>,
    cover_path: Option<PathBuf>,
}

#[derive(Default)]
struct SettingsState {
    open: bool,
    tmdb: String,
    igdb_id: String,
    igdb_secret: String,
    drinks_url: String,
    auto_update: bool,
    message: String,
}

#[derive(Default)]
struct TextExportState {
    open: bool,
    markdown: bool,
    fields: Vec<(String, String, bool)>, // key, label, checked
}

#[derive(Default)]
struct ImageExportState {
    open: bool,
    list_id: u64,
    included: Vec<bool>,
    order: Vec<usize>,
    arrangement: Arrangement,
    fit: FitMode,
    format: ImageFormat,
    message: String,
}

#[derive(Default)]
struct BackupState {
    open: bool,
    scope_idx: usize, // 0 single, 1 selected, 2 whole, 3 category
    selected: Vec<(u64, bool)>,
    category: Category,
    message: String,
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

pub struct FaveApp {
    base_dir: PathBuf,
    store: Store,
    // `pub(crate)` on the fields `app_bulk.rs` (Slice 3) shares: it holds
    // the bulk-selection bar, the duplicates view, and the grid-drop
    // handling as a second `impl FaveApp` block.
    pub(crate) library: Library,
    settings: Settings,
    drinks: DrinksDb,
    pub(crate) selected_list: Option<u64>,
    pub(crate) selected_item: Option<u64>,
    filter: String,
    pub(crate) status: String,
    // bulk selection + duplicates (v0.2 #9 / #3, see app_bulk.rs)
    pub(crate) bulk_selection: Vec<u64>,
    pub(crate) select_mode: bool,
    pub(crate) dup: crate::app_bulk::DuplicatesState,
    /// Sidebar list-row rects from the last frame, for drag-to-list drops.
    pub(crate) sidebar_list_rects: Vec<(u64, egui::Rect)>,
    // undo/redo (v0.2 #4): the session history the registry records into
    history: crate::commands::History,
    // global search (v0.2 #2)
    global_query: String,
    global_hits: Vec<crate::search::SearchHit>,
    /// The exact query the cached `global_hits` were computed for.
    global_hits_query: String,
    global_focus_request: bool,
    // search
    search_query: String,
    search_for_list: Option<u64>,
    suggestions: Vec<SuggestionView>,
    searching: bool,
    search_error: Option<String>,
    last_query_change: Instant,
    sent_query: String,
    // detail editing
    edit_title: String,
    edit_fields: BTreeMap<String, String>,
    // personal rating + note (v0.2 #5)
    edit_rating: Option<u8>,
    edit_note: String,
    editing_item: Option<u64>,
    // textures
    cover_textures: HashMap<String, egui::TextureHandle>,
    thumb_textures: HashMap<String, egui::TextureHandle>,
    // dialogs
    new_list: NewListState,
    manual: ManualState,
    settings_ui: SettingsState,
    text_export: TextExportState,
    image_export: ImageExportState,
    backup_ui: BackupState,
    // share card + CSV import (v0.2 #8 / #10, see app_share.rs)
    pub(crate) share_ui: crate::app_share::ShareCardState,
    pub(crate) csv_ui: crate::app_share::CsvImportState,
    confirm_delete_list: Option<u64>,
    // background plumbing
    tx: Sender<BgMsg>,
    rx: Receiver<BgMsg>,
    pending_add: bool,
    pub(crate) drag_source: Option<usize>,
    drinks_check_started: bool,
}

impl FaveApp {
    pub fn new(base_dir: PathBuf) -> FaveApp {
        let store = Store::open(&base_dir).unwrap_or_else(|_| {
            Store::open_at(&base_dir.join(crate::store::DATA_DIR_NAME)).expect("create data folder")
        });
        // Journal-recovering load (#4): a corrupt/missing main library file
        // falls back to the autosave journal, and the user is told.
        let load = store.load_library_recovering().unwrap_or_default();
        let recovered = load.recovered_from_journal;
        let library = load.library;
        let settings = store.load_settings().unwrap_or_default();
        let drinks = DrinksDb::load_effective(&store.data_dir);
        let (tx, rx) = channel();
        FaveApp {
            base_dir,
            store,
            library,
            settings,
            drinks,
            selected_list: None,
            selected_item: None,
            bulk_selection: Vec::new(),
            select_mode: false,
            dup: crate::app_bulk::DuplicatesState::default(),
            sidebar_list_rects: Vec::new(),
            filter: String::new(),
            status: if recovered {
                "Recovered your library from the autosave journal.".to_string()
            } else {
                "Welcome to FaveLists.".to_string()
            },
            history: crate::commands::History::new(),
            global_query: String::new(),
            global_hits: Vec::new(),
            global_hits_query: String::new(),
            global_focus_request: false,
            search_query: String::new(),
            search_for_list: None,
            suggestions: Vec::new(),
            searching: false,
            search_error: None,
            last_query_change: Instant::now(),
            sent_query: String::new(),
            edit_title: String::new(),
            edit_fields: BTreeMap::new(),
            edit_rating: None,
            edit_note: String::new(),
            editing_item: None,
            cover_textures: HashMap::new(),
            thumb_textures: HashMap::new(),
            new_list: NewListState::default(),
            manual: ManualState::default(),
            settings_ui: SettingsState::default(),
            text_export: TextExportState::default(),
            image_export: ImageExportState::default(),
            backup_ui: BackupState::default(),
            share_ui: crate::app_share::ShareCardState::default(),
            csv_ui: crate::app_share::CsvImportState::default(),
            confirm_delete_list: None,
            tx,
            rx,
            pending_add: false,
            drag_source: None,
            drinks_check_started: false,
        }
    }

    pub(crate) fn persist(&mut self) {
        if let Err(e) = self.store.save_library(&self.library) {
            self.status = format!("Could not save library: {e}");
        }
        if let Err(e) = self.store.save_settings(&self.settings) {
            self.status = format!("Could not save settings: {e}");
        }
    }

    /// Route a core operation through the command registry (v0.2 program
    /// #1) — the same dispatch the headless JSON channel uses. The registry
    /// mutates `self.library`; persistence stays with the caller
    /// (`persist()`), exactly as before routing.
    pub(crate) fn dispatch(
        &mut self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, crate::commands::CommandError> {
        let covers = self.store.covers_dir();
        let mut ctx = crate::commands::CommandContext::with_history(
            &mut self.library,
            &covers,
            &mut self.history,
        );
        crate::commands::dispatch(&mut ctx, name, &params)
    }

    fn selected_list_ref(&self) -> Option<&FavList> {
        self.selected_list.and_then(|id| self.library.list(id))
    }

    // -- covers -------------------------------------------------------------

    fn cover_texture(&mut self, ctx: &egui::Context, cover: &str) -> Option<egui::TextureHandle> {
        if let Some(t) = self.cover_textures.get(cover) {
            return Some(t.clone());
        }
        let path = self.store.covers_dir().join(cover);
        let img = image::open(&path).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let color =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
        let tex = ctx.load_texture(
            format!("cover:{cover}"),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.cover_textures.insert(cover.to_string(), tex.clone());
        Some(tex)
    }

    fn thumb_texture(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        image: &egui::ColorImage,
    ) -> egui::TextureHandle {
        if let Some(t) = self.thumb_textures.get(key) {
            return t.clone();
        }
        let tex = ctx.load_texture(
            format!("thumb:{key}"),
            image.clone(),
            egui::TextureOptions::LINEAR,
        );
        self.thumb_textures.insert(key.to_string(), tex.clone());
        tex
    }

    // -- background work ----------------------------------------------------

    fn maybe_start_drinks_auto_update(&mut self) {
        if self.drinks_check_started {
            return;
        }
        self.drinks_check_started = true;
        if !self.settings.drinks_auto_update || self.settings.drinks_update_url.trim().is_empty() {
            return;
        }
        let now = crate::model::now_unix();
        if now - self.settings.drinks_last_check < 24 * 3600 {
            return;
        }
        self.start_drinks_update();
    }

    fn start_drinks_update(&mut self) {
        let url = self.settings.drinks_update_url.trim().to_string();
        if url.is_empty() {
            self.settings_ui.message =
                "Set a drinks database URL first (the update URL is blank).".to_string();
            return;
        }
        let tx = self.tx.clone();
        let current_version = self.drinks.version;
        std::thread::spawn(move || {
            let result = crate::drinks::fetch_update(&url).and_then(|pack| {
                let current = DrinksDb {
                    version: current_version,
                    source: String::new(),
                    entries: Vec::new(),
                };
                crate::drinks::validate_update(&current, &pack).map(|_| pack)
            });
            // The validated pack travels back serialised in the OK payload.
            let _ = tx.send(BgMsg::DrinksUpdate(match result {
                Ok(pack) => Ok(serde_json::to_string(&pack).unwrap_or_default()),
                Err(e) => Err(e),
            }));
        });
    }

    fn poll_background(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                BgMsg::Search {
                    list_id,
                    query,
                    result,
                } => {
                    if self.search_for_list == Some(list_id) && query == self.search_query.trim() {
                        self.searching = false;
                        match result {
                            Ok(views) => {
                                self.search_error = None;
                                self.suggestions = views;
                            }
                            Err(e) => {
                                self.search_error = Some(e);
                                self.suggestions.clear();
                            }
                        }
                    }
                }
                BgMsg::AddReady {
                    list_id,
                    title,
                    item_fields,
                    provider_id,
                    source,
                    cover_bytes,
                } => {
                    self.pending_add = false;
                    let pending = PendingAdd {
                        title,
                        fields: item_fields,
                        provider_id,
                        source,
                        cover_bytes,
                    };
                    self.apply_pending_add(list_id, pending);
                }
                BgMsg::DrinksUpdate(result) => {
                    self.settings.drinks_last_check = crate::model::now_unix();
                    match result {
                        Ok(pack_json) => {
                            if let Ok(pack) = serde_json::from_str::<DrinksDb>(&pack_json) {
                                let path = self.store.drinks_path();
                                if crate::store::save_json_atomic(&path, &pack).is_ok() {
                                    self.drinks = pack;
                                    self.status = format!(
                                        "Drinks database updated ({} entries).",
                                        self.drinks.entries.len()
                                    );
                                    self.settings_ui.message = self.status.clone();
                                }
                            }
                        }
                        Err(e) => {
                            self.settings_ui.message = format!("Drinks database update: {e}");
                        }
                    }
                    self.persist();
                }
            }
        }
    }

    fn apply_pending_add(&mut self, list_id: u64, pending: PendingAdd) {
        let title = pending.title.clone();
        let fields = serde_json::to_value(&pending.fields).unwrap_or_default();
        let mut params = serde_json::json!({
            "list_id": list_id,
            "title": pending.title,
            "fields": fields,
            "provider_id": pending.provider_id,
            "source": pending.source,
        });
        if let Some(bytes) = &pending.cover_bytes {
            let ext = image::guess_format(bytes)
                .map(|f| match f {
                    image::ImageFormat::Png => "png",
                    image::ImageFormat::WebP => "webp",
                    _ => "jpg",
                })
                .unwrap_or("jpg");
            params["cover_bytes_base64"] =
                serde_json::Value::String(crate::commands::base64_encode(bytes));
            params["cover_ext"] = serde_json::Value::String(ext.to_string());
        }
        // Preserve the pre-routing behaviour on a cover-write failure (the
        // item was still added, just without its cover): retry once with
        // the cover bytes stripped.
        let mut bare = params.clone();
        if let Some(o) = bare.as_object_mut() {
            o.remove("cover_bytes_base64");
            o.remove("cover_ext");
        }
        let result = match self.dispatch("item.add", params) {
            Err(e) if e.code == "io" && pending.cover_bytes.is_some() => {
                self.dispatch("item.add", bare)
            }
            other => other,
        };
        match result {
            Ok(r) if r["outcome"] == "duplicate" => {
                self.status = format!("{title} is already in this list.");
            }
            Ok(_) => {
                self.status = format!("Added {title}.");
                self.search_query.clear();
                self.suggestions.clear();
                self.sent_query.clear();
            }
            Err(_) => {
                // The list vanished between pick and add; nothing to do.
            }
        }
        self.persist();
    }

    /// Kick off a provider search for the current query (debounced by the
    /// caller in `update`).
    fn start_search(&mut self, list_id: u64, query: String) {
        let Some(list) = self.library.list(list_id) else {
            return;
        };
        let category = list.category;
        self.searching = true;
        self.search_error = None;
        self.sent_query = query.clone();
        // Drinks: local, instant, still funnelled through the same message
        // shape for a uniform UI.
        if let Some(kind) = category.drinks_kind() {
            let views = self
                .drinks
                .search(kind, &query, providers::MAX_RESULTS)
                .into_iter()
                .map(|e| SuggestionView {
                    suggestion: suggestion_from_drink(e),
                    thumb: None,
                })
                .collect();
            self.suggestions = views;
            self.searching = false;
            return;
        }
        if let Err(e) = providers::availability(category, &self.settings) {
            self.searching = false;
            self.search_error = Some(e);
            self.suggestions.clear();
            return;
        }
        let tx = self.tx.clone();
        let settings = self.settings.clone();
        std::thread::spawn(move || {
            let result = providers::search(category, &query, &settings).map(|sugs| {
                sugs.into_iter()
                    .map(|sug| {
                        let thumb = sug.thumb_url.as_deref().and_then(|u| {
                            providers::download_bytes(u).ok().and_then(|bytes| {
                                image::load_from_memory(&bytes).ok().map(|img| {
                                    let small = img.thumbnail(92, 138).to_rgba8();
                                    let (w, h) = small.dimensions();
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        [w as usize, h as usize],
                                        small.as_raw(),
                                    )
                                })
                            })
                        });
                        SuggestionView {
                            suggestion: sug,
                            thumb,
                        }
                    })
                    .collect::<Vec<_>>()
            });
            let _ = tx.send(BgMsg::Search {
                list_id,
                query,
                result,
            });
        });
    }

    /// User picked a suggestion: fetch details + cover on a thread, then add.
    fn pick_suggestion(&mut self, list_id: u64, view_idx: usize) {
        let Some(view) = self.suggestions.get(view_idx) else {
            return;
        };
        let sug = view.suggestion.clone();
        let Some(list) = self.library.list(list_id) else {
            return;
        };
        let category = list.category;
        // Drinks entries are already complete.
        if category.drinks_kind().is_some() {
            let pending = PendingAdd {
                title: sug.title.clone(),
                fields: sug.fields.clone(),
                provider_id: Some(sug.provider_id.clone()),
                source: sug.source.to_string(),
                cover_bytes: sug
                    .cover_url
                    .as_deref()
                    .and_then(|u| providers::download_bytes(u).ok()),
            };
            self.apply_pending_add(list_id, pending);
            return;
        }
        self.pending_add = true;
        self.status = format!("Adding {}…", sug.title);
        let tx = self.tx.clone();
        let settings = self.settings.clone();
        std::thread::spawn(move || {
            let mut fields = sug.fields.clone();
            for (k, v) in providers::fetch_details(category, &sug, &settings) {
                fields.entry(k).or_insert(v);
            }
            let cover_url = providers::details_cover_url(category, &sug, &settings)
                .or_else(|| sug.cover_url.clone());
            let mut cover_bytes = cover_url
                .as_deref()
                .and_then(|u| providers::download_bytes(u).ok());
            if cover_bytes.is_none() && category == Category::Artists {
                // #6 fallback: MusicBrainz has no artist photos, so try
                // the key-free Deezer → Wikipedia chain by artist name.
                // Still best-effort — a miss leaves the item coverless,
                // exactly as before, and manual upload is unaffected.
                cover_bytes = providers::artist_image::fetch_artist_image(&sug.title);
            }
            let _ = tx.send(BgMsg::AddReady {
                list_id,
                title: sug.title.clone(),
                item_fields: fields,
                provider_id: Some(sug.provider_id.clone()),
                source: sug.source.to_string(),
                cover_bytes,
            });
        });
    }
}

fn suggestion_from_drink(e: crate::drinks::DrinkEntry) -> Suggestion {
    let mut fields = BTreeMap::new();
    if !e.brand.is_empty() {
        fields.insert("brand".to_string(), e.brand.clone());
    }
    if !e.flavor.is_empty() {
        fields.insert("flavor".to_string(), e.flavor.clone());
    }
    // v0.2 #7: surface caffeine from the pack when the entry carries it
    // (e.g. "32 mg/100ml"); lands in the entry's free `fields` bag under
    // the `caffeine` key, like brand/flavor.
    if let Some(label) = e.caffeine_label() {
        fields.insert("caffeine".to_string(), label);
    }
    Suggestion {
        provider_id: format!("{}:{}", e.brand, e.name),
        source: "drinks-db",
        title: e.name.clone(),
        year: None,
        detail: e.detail_line(),
        thumb_url: e.image_url.clone(),
        cover_url: e.image_url.clone(),
        fields,
    }
}

pub fn kind_of(category: Category) -> Option<DrinkKind> {
    category.drinks_kind()
}

/// Stable machine id for an arrangement — the `export.image` command's
/// `arrangement` param (the registry parses these back).
fn arrangement_id(a: Arrangement) -> &'static str {
    match a {
        Arrangement::UniformGrid => "uniform_grid",
        Arrangement::Topster => "topster",
        Arrangement::Mosaic => "mosaic",
        Arrangement::ShelfRows => "shelf_rows",
        Arrangement::Scattered => "scattered",
        Arrangement::Spiral => "spiral",
        Arrangement::Honeycomb => "honeycomb",
        Arrangement::Ring => "ring",
        Arrangement::Filmstrip => "filmstrip",
        Arrangement::Masonry => "masonry",
    }
}

// ---------------------------------------------------------------------------
// Frame rendering
// ---------------------------------------------------------------------------

const TILE_W: f32 = 138.0;
const TILE_H: f32 = 207.0;

impl eframe::App for FaveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        install_theme(ctx);
        self.maybe_start_drinks_auto_update();
        self.poll_background();
        self.maybe_fire_search(ctx);
        self.handle_shortcuts(ctx);
        self.refresh_global_results();

        egui::SidePanel::left("sidebar")
            .default_width(230.0)
            .show(ctx, |ui| self.sidebar_ui(ui));

        if self.selected_item.is_some() {
            egui::SidePanel::right("detail")
                .default_width(280.0)
                .show(ctx, |ui| self.detail_ui(ui));
        }

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).color(COL_MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} lists · {} items · data: {}",
                            self.library.lists.len(),
                            self.library.total_items(),
                            self.store.data_dir.display()
                        ))
                        .color(COL_MUTED)
                        .small(),
                    );
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.dup.open {
                self.duplicates_ui(ui);
            } else if self.global_search_active() {
                self.global_results_ui(ui);
            } else {
                self.main_ui(ui);
            }
        });

        self.new_list_dialog(ctx);
        self.manual_dialog(ctx);
        self.settings_dialog(ctx);
        self.text_export_dialog(ctx);
        self.image_export_dialog(ctx);
        self.backup_dialog(ctx);
        self.share_card_dialog(ctx);
        self.csv_import_dialog(ctx);
        if self.searching || self.pending_add {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

impl FaveApp {
    fn maybe_fire_search(&mut self, _ctx: &egui::Context) {
        let Some(list_id) = self.selected_list else {
            return;
        };
        if self.search_for_list != Some(list_id) {
            self.search_for_list = Some(list_id);
            self.search_query.clear();
            self.suggestions.clear();
            self.sent_query.clear();
        }
        let q = self.search_query.trim().to_string();
        let q_len = q.len();
        if q_len >= 2
            && q != self.sent_query
            && !self.searching
            && self.last_query_change.elapsed() > std::time::Duration::from_millis(350)
        {
            self.start_search(list_id, q);
        }
        if q_len < 2 && !self.suggestions.is_empty() {
            self.suggestions.clear();
            self.sent_query.clear();
        }
    }

    // -- global shortcuts: Ctrl+K search focus, Ctrl+Z / Ctrl+Y undo/redo --

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            self.global_focus_request = true;
        }
        // Redo combos first so Ctrl+Shift+Z isn't swallowed by the undo one.
        let redo = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
        });
        if redo {
            self.apply_history_command("history.redo", "redone", "Redo");
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.apply_history_command("history.undo", "undone", "Undo");
        }
    }

    /// Dispatch `history.undo`/`history.redo` and report the outcome in the
    /// status line, persisting like any other mutation.
    fn apply_history_command(&mut self, name: &str, flag: &str, verb: &str) {
        match self.dispatch(name, serde_json::json!({})) {
            Ok(result) => {
                if result[flag].as_bool() == Some(true) {
                    let cmd = result["command"].as_str().unwrap_or("change");
                    self.status = format!("{verb}: {cmd}.");
                } else {
                    self.status = format!("Nothing to {}.", verb.to_lowercase());
                }
            }
            Err(e) => {
                self.status = format!("{verb} failed: {e}");
            }
        }
        self.repair_selection();
        // The library changed under any active global search: force the
        // cached hits to recompute on the next frame.
        self.global_hits_query.clear();
        self.persist();
    }

    /// After an undo/redo the selection may point at a list or item that
    /// no longer exists; drop dangling selections.
    pub(crate) fn repair_selection(&mut self) {
        if let Some(list_id) = self.selected_list {
            match self.library.list(list_id) {
                None => {
                    self.selected_list = None;
                    self.selected_item = None;
                }
                Some(list) => {
                    if let Some(item_id) = self.selected_item {
                        if list.item(item_id).is_none() {
                            self.selected_item = None;
                            self.editing_item = None;
                        }
                    }
                }
            }
        }
    }

    // -- global search (#2) -------------------------------------------------

    fn global_search_active(&self) -> bool {
        !self.global_query.trim().is_empty()
    }

    /// Recompute the cached hits when the query changed. The query itself
    /// goes through the `library.search` command — never an inline filter.
    fn refresh_global_results(&mut self) {
        let q = self.global_query.trim().to_string();
        if q == self.global_hits_query {
            return;
        }
        self.global_hits_query = q.clone();
        if q.is_empty() {
            self.global_hits.clear();
            return;
        }
        match self.dispatch("library.search", serde_json::json!({"query": q})) {
            Ok(result) => self.global_hits = crate::search::hits_from_result(&result),
            Err(_) => self.global_hits.clear(),
        }
    }

    fn global_results_ui(&mut self, ui: &mut egui::Ui) {
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.heading(format!("Search results for “{}”", self.global_query.trim()));
            ui.label(
                egui::RichText::new(format!("{} found", self.global_hits.len())).color(COL_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕ Clear search (Esc)").clicked() {
                    clear = true;
                }
            });
        });
        if clear {
            self.global_query.clear();
            return;
        }
        ui.separator();
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.global_query.clear();
            return;
        }
        if self.global_hits.is_empty() {
            ui.label(egui::RichText::new("No matches in any list.").color(COL_MUTED));
            return;
        }
        let groups = crate::search::grouped(&self.global_hits);
        let mut jump: Option<(u64, u64)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (list_id, idxs) in &groups {
                let first = &self.global_hits[idxs[0]];
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{} · {}", first.list_name, first.category.name()))
                        .strong(),
                );
                for &hi in idxs {
                    let hit = &self.global_hits[hi];
                    let label = match &hit.year {
                        Some(y) => format!("{} ({})", hit.title, y),
                        None => hit.title.clone(),
                    };
                    if ui.selectable_label(false, label).clicked() {
                        jump = Some((*list_id, hit.item_id));
                    }
                }
            }
        });
        if let Some((list_id, item_id)) = jump {
            self.selected_list = Some(list_id);
            self.bulk_selection.clear();
            self.filter.clear();
            self.global_query.clear();
            self.select_item(item_id);
        }
    }

    fn sidebar_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("FaveLists");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ Settings").clicked() {
                    self.settings_ui = SettingsState {
                        open: true,
                        tmdb: self.settings.tmdb_api_key.clone(),
                        igdb_id: self.settings.igdb_client_id.clone(),
                        igdb_secret: self.settings.igdb_client_secret.clone(),
                        drinks_url: self.settings.drinks_update_url.clone(),
                        auto_update: self.settings.drinks_auto_update,
                        message: String::new(),
                    };
                }
            });
        });
        ui.separator();
        if ui.button("＋ New list").clicked() {
            self.new_list.open = true;
        }
        if ui.button("⧉ Find duplicates").clicked() {
            self.open_duplicates();
        }
        if ui.button("⇪ Backup / Restore").clicked() {
            self.backup_ui.open = true;
            self.backup_ui.selected = self.library.lists.iter().map(|l| (l.id, false)).collect();
            self.backup_ui.message.clear();
        }
        if ui.button("⇩ Import CSV…").clicked() {
            self.open_csv_import();
        }
        // Global search (#2): searches every list via the library.search
        // command; results replace the main area while the query is active.
        let search_resp = ui.add(
            egui::TextEdit::singleline(&mut self.global_query)
                .desired_width(ui.available_width())
                .hint_text("Search all lists… (Ctrl+K)"),
        );
        if self.global_focus_request {
            search_resp.request_focus();
            self.global_focus_request = false;
        }
        ui.separator();
        // Record this frame's list-row rects so a grid drag released over
        // a row can drop the item onto that list (#9, see app_bulk.rs).
        self.sidebar_list_rects.clear();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for cat in Category::ALL {
                let lists = self.library.lists_in_category(cat);
                ui.collapsing(cat.name(), |ui| {
                    if lists.is_empty() {
                        ui.label(egui::RichText::new("No lists yet").color(COL_MUTED).small());
                    }
                    for list in lists {
                        let label = format!("{} ({})", list.name, list.items.len());
                        let selected = self.selected_list == Some(list.id);
                        let resp = ui.selectable_label(selected, label);
                        self.sidebar_list_rects.push((list.id, resp.rect));
                        if resp.clicked() {
                            self.selected_list = Some(list.id);
                            self.selected_item = None;
                            self.bulk_selection.clear();
                            self.filter.clear();
                        }
                    }
                });
            }
        });
    }

    fn main_ui(&mut self, ui: &mut egui::Ui) {
        let Some(list_id) = self.selected_list else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.heading("Your favorite things, beautifully listed.");
                ui.add_space(8.0);
                ui.label("Create a list from the sidebar to get started.");
                ui.label(
                    egui::RichText::new(
                        "Lists are single-type: movies, shows, books, music, games, \
                         podcasts, board games, energy drinks, or sodas.",
                    )
                    .color(COL_MUTED),
                );
            });
            return;
        };
        let Some(list) = self.library.list(list_id).cloned() else {
            self.selected_list = None;
            return;
        };
        // Header: name, ranked switch, sorts, exports, delete.
        ui.horizontal_wrapped(|ui| {
            let mut name = list.name.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(220.0))
                .changed()
            {
                if let Some(l) = self.library.list_mut(list_id) {
                    l.name = name;
                }
                self.persist();
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} · {} items",
                    list.category.name(),
                    list.items.len()
                ))
                .color(COL_MUTED),
            );
            let mut ranked = list.ranked;
            let ranked_label = if ranked { "Ranked" } else { "Unranked" };
            if ui.checkbox(&mut ranked, ranked_label).changed() {
                let _ = self.dispatch(
                    "list.set_ranked",
                    serde_json::json!({"list_id": list_id, "ranked": ranked}),
                );
                self.persist();
            }
            if ui.button("Sort A–Z").clicked() {
                self.sort_list(list_id, SortKind::TitleAZ);
            }
            if ui.button("Sort by year").clicked() {
                self.sort_list(list_id, SortKind::Year);
            }
            if ui.button("Sort by added").clicked() {
                self.sort_list(list_id, SortKind::DateAdded);
            }
            if ui.button("Sort by rating").clicked() {
                self.sort_list(list_id, SortKind::Rating);
            }
            if ui.button("Export text…").clicked() {
                self.open_text_export(&list);
            }
            if ui.button("Export image…").clicked() {
                self.open_image_export(&list);
            }
            if ui.button("Share card…").clicked() {
                self.open_share_card(&list);
            }
            if self.confirm_delete_list == Some(list_id) {
                if ui
                    .button(egui::RichText::new("Confirm delete list?").color(COL_ACCENT))
                    .clicked()
                {
                    let _ = self.dispatch("list.delete", serde_json::json!({"list_id": list_id}));
                    self.selected_list = None;
                    self.selected_item = None;
                    self.confirm_delete_list = None;
                    self.persist();
                    self.status = "List deleted.".to_string();
                }
            } else if ui.button("Delete list").clicked() {
                self.confirm_delete_list = Some(list_id);
            }
        });
        ui.separator();
        // Add-item search.
        ui.horizontal(|ui| {
            ui.label("Add:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .desired_width(340.0)
                    .hint_text(format!("Start typing a {} title…", list.category.name())),
            );
            if resp.changed() {
                self.last_query_change = Instant::now();
            }
            if self.searching {
                ui.spinner();
                ui.label("Searching…");
            }
            if self.pending_add {
                ui.label("Adding…");
            }
            if ui.button("Add manually").clicked() {
                self.manual.open = true;
                self.manual.title = self.search_query.trim().to_string();
                self.manual.fields = BTreeMap::new();
                self.manual.cover_path = None;
            }
        });
        if let Some(err) = &self.search_error {
            ui.label(egui::RichText::new(err).color(COL_ACCENT).small());
        }
        if !self.suggestions.is_empty() {
            self.suggestions_ui(ui, list_id);
        }
        ui.separator();
        // Filter + grid.
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(200.0)
                    .hint_text("Filter this list…"),
            );
            // Bulk selection (#9): with Select on, plain clicks toggle
            // items in/out of the bulk selection; Ctrl+click toggles in
            // either mode. The bulk action bar appears in app_bulk.rs.
            ui.toggle_value(&mut self.select_mode, "☑ Select");
            if !self.bulk_selection.is_empty() {
                ui.label(
                    egui::RichText::new(format!("{} selected", self.bulk_selection.len()))
                        .color(COL_MUTED)
                        .small(),
                );
            }
        });
        // Drop bulk-selected ids that are no longer in this list (the
        // items were removed, merged away, or undone since selecting).
        self.bulk_selection
            .retain(|id| list.items.iter().any(|i| i.id == *id));
        self.bulk_bar_ui(ui, &list);
        ui.add_space(4.0);
        if list.items.is_empty() {
            ui.label(
                egui::RichText::new("This list is empty. Search above to add your first favorite.")
                    .color(COL_MUTED),
            );
            return;
        }
        self.grid_ui(ui, &list);
    }

    fn sort_list(&mut self, list_id: u64, kind: SortKind) {
        let kind = match kind {
            SortKind::TitleAZ => "title",
            SortKind::Year => "year",
            SortKind::DateAdded => "added",
            SortKind::Rating => "rating",
        };
        let _ = self.dispatch(
            "list.sort",
            serde_json::json!({"list_id": list_id, "kind": kind}),
        );
        self.persist();
    }

    fn suggestions_ui(&mut self, ui: &mut egui::Ui, list_id: u64) {
        let mut picked: Option<usize> = None;
        // Take the suggestions out so texture creation (&mut self) doesn't
        // fight the iteration borrow; put them back afterwards.
        let views = std::mem::take(&mut self.suggestions);
        egui::Frame::default()
            .fill(COL_PANEL)
            .stroke(egui::Stroke::new(1.0f32, COL_HAIRLINE))
            .inner_margin(6.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (i, view) in views.iter().enumerate() {
                            let sug = &view.suggestion;
                            let resp = ui
                                .horizontal(|ui| {
                                    if let Some(img) = &view.thumb {
                                        let tex = self.thumb_texture(
                                            ui.ctx(),
                                            &format!("{}:{}", sug.source, sug.provider_id),
                                            img,
                                        );
                                        ui.image((tex.id(), egui::vec2(34.0, 51.0)));
                                    } else {
                                        ui.allocate_exact_size(
                                            egui::vec2(34.0, 51.0),
                                            egui::Sense::hover(),
                                        );
                                    }
                                    ui.vertical(|ui| {
                                        let title_line = match &sug.year {
                                            Some(y) => format!("{} ({})", sug.title, y),
                                            None => sug.title.clone(),
                                        };
                                        ui.label(egui::RichText::new(title_line).strong());
                                        if !sug.detail.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&sug.detail)
                                                    .color(COL_MUTED)
                                                    .small(),
                                            );
                                        }
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());
                            if resp.clicked() {
                                picked = Some(i);
                            }
                        }
                    });
            });
        self.suggestions = views;
        if let Some(i) = picked {
            self.pick_suggestion(list_id, i);
        }
    }

    fn grid_ui(&mut self, ui: &mut egui::Ui, list: &FavList) {
        let filter = self.filter.trim().to_lowercase();
        let visible: Vec<usize> = list
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| filter.is_empty() || it.title.to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect();
        let cols = ((ui.available_width() / (TILE_W + 10.0)) as usize).max(1);
        let mut clicked: Option<u64> = None;
        let mut toggled: Option<u64> = None;
        // Ctrl+click toggles bulk selection in any mode; in Select mode
        // every click toggles (#9).
        let toggle_clicks = self.select_mode || ui.ctx().input(|i| i.modifiers.ctrl);
        let mut tile_rects: Vec<(usize, egui::Rect)> = Vec::new();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for chunk in visible.chunks(cols) {
                ui.horizontal(|ui| {
                    for &idx in chunk {
                        let item = &list.items[idx];
                        ui.vertical(|ui| {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(TILE_W, TILE_H),
                                egui::Sense::click_and_drag(),
                            );
                            tile_rects.push((idx, rect));
                            let painter = ui.painter_at(rect);
                            painter.rect_filled(rect, 4.0, COL_PANEL);
                            painter.rect_stroke(
                                rect,
                                4.0,
                                egui::Stroke::new(1.0f32, COL_HAIRLINE),
                                egui::StrokeKind::Inside,
                            );
                            if let Some(cover) = &item.cover {
                                if let Some(tex) = self.cover_texture(ui.ctx(), cover) {
                                    painter.image(
                                        tex.id(),
                                        rect.shrink(3.0),
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                }
                            } else {
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "no cover",
                                    egui::FontId::proportional(13.0),
                                    COL_MUTED,
                                );
                            }
                            if list.ranked {
                                let badge = egui::Rect::from_min_size(
                                    rect.min + egui::vec2(6.0, 6.0),
                                    egui::vec2(30.0, 22.0),
                                );
                                painter.rect_filled(badge, 4.0, COL_DEEP_RED);
                                painter.text(
                                    badge.center(),
                                    egui::Align2::CENTER_CENTER,
                                    format!("#{}", idx + 1),
                                    egui::FontId::proportional(13.0),
                                    COL_TEXT,
                                );
                            }
                            if self.selected_item == Some(item.id) {
                                painter.rect_stroke(
                                    rect,
                                    4.0,
                                    egui::Stroke::new(2.0f32, COL_ACCENT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if self.bulk_selection.contains(&item.id) {
                                painter.rect_stroke(
                                    rect,
                                    4.0,
                                    egui::Stroke::new(2.0f32, COL_ACCENT),
                                    egui::StrokeKind::Inside,
                                );
                                let badge = egui::Rect::from_min_size(
                                    egui::pos2(rect.max.x - 24.0, rect.min.y + 4.0),
                                    egui::vec2(20.0, 20.0),
                                );
                                painter.rect_filled(badge, 4.0, COL_DEEP_RED);
                                painter.text(
                                    badge.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "✓",
                                    egui::FontId::proportional(13.0),
                                    COL_TEXT,
                                );
                            }
                            if resp.clicked() {
                                if toggle_clicks {
                                    toggled = Some(item.id);
                                } else {
                                    clicked = Some(item.id);
                                }
                            }
                            if resp.drag_started() {
                                self.drag_source = Some(idx);
                            }
                            let title = if item.title.chars().count() > 20 {
                                format!("{}…", item.title.chars().take(19).collect::<String>())
                            } else {
                                item.title.clone()
                            };
                            ui.add_sized(
                                egui::vec2(TILE_W, 18.0),
                                egui::Label::new(egui::RichText::new(title).small())
                                    .halign(egui::Align::Center),
                            );
                        });
                    }
                });
            }
        });
        if let Some(id) = toggled {
            self.toggle_bulk(id);
        }
        if let Some(id) = clicked {
            self.select_item(id);
        }
        // Drag release: reorder within the grid, or drag-to-list onto a
        // sidebar row — both live in app_bulk.rs (#9).
        self.grid_drop(ui, list, &tile_rects);
    }

    fn select_item(&mut self, item_id: u64) {
        self.selected_item = Some(item_id);
        if self.editing_item != Some(item_id) {
            let found = self.selected_list_ref().and_then(|list| {
                list.item(item_id).map(|item| {
                    (
                        item.title.clone(),
                        item.fields.clone(),
                        item.rating,
                        item.note.clone(),
                    )
                })
            });
            if let Some((title, fields, rating, note)) = found {
                self.edit_title = title;
                self.edit_fields = fields;
                self.edit_rating = rating;
                self.edit_note = note;
                self.editing_item = Some(item_id);
            }
        }
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui) {
        let Some(item_id) = self.selected_item else {
            return;
        };
        let Some(list) = self.selected_list_ref().cloned() else {
            return;
        };
        let Some(item) = list.item(item_id).cloned() else {
            self.selected_item = None;
            return;
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Details");
            if let Some(r) = item.rating {
                ui.label(
                    egui::RichText::new(format!("★ {r}/10"))
                        .color(COL_ACCENT)
                        .strong(),
                );
            }
        });
        ui.separator();
        if let Some(cover) = &item.cover {
            if let Some(tex) = self.cover_texture(ui.ctx(), cover) {
                ui.image((tex.id(), egui::vec2(180.0, 270.0)));
            }
        } else {
            ui.label(egui::RichText::new("No cover image").color(COL_MUTED));
        }
        if ui.button("Replace cover…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
                .pick_file()
            {
                self.replace_cover(list.id, item_id, &path);
            }
        }
        ui.add_space(4.0);
        ui.label("Title");
        ui.text_edit_singleline(&mut self.edit_title);
        for f in list.category.fields() {
            ui.label(f.label);
            let entry = self.edit_fields.entry(f.key.to_string()).or_default();
            ui.text_edit_singleline(entry);
        }
        // Personal rating + note (v0.2 #5): saved through the same
        // item.edit dispatch below, so both are undoable.
        ui.label("Rating");
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.edit_rating.is_none(), "—")
                .clicked()
            {
                self.edit_rating = None;
            }
            for r in 1..=10u8 {
                if ui
                    .selectable_label(self.edit_rating == Some(r), r.to_string())
                    .clicked()
                {
                    self.edit_rating = Some(r);
                }
            }
        });
        ui.label("Note");
        ui.add(
            egui::TextEdit::multiline(&mut self.edit_note)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.label(
            egui::RichText::new(format!("Source: {}", item.source))
                .color(COL_MUTED)
                .small(),
        );
        ui.add_space(6.0);
        if ui.button("Save changes").clicked() {
            // The registry applies the same trim/drop-empty normalisation
            // this panel used to do inline.
            let fields = serde_json::to_value(&self.edit_fields).unwrap_or_default();
            let _ = self.dispatch(
                "item.edit",
                serde_json::json!({
                    "list_id": list.id,
                    "item_id": item_id,
                    "title": self.edit_title.trim(),
                    "fields": fields,
                    "rating": self.edit_rating,
                    "note": self.edit_note,
                }),
            );
            self.cover_textures.clear();
            self.persist();
            self.status = "Item updated.".to_string();
        }
        if ui
            .button(egui::RichText::new("Remove from list").color(COL_ACCENT))
            .clicked()
        {
            let _ = self.dispatch(
                "item.remove",
                serde_json::json!({"list_id": list.id, "item_id": item_id}),
            );
            self.selected_item = None;
            self.persist();
            self.status = "Item removed.".to_string();
        }
    }

    fn replace_cover(&mut self, list_id: u64, item_id: u64, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_lowercase();
        let name = format!("cover-{item_id}.{ext}");
        let dest = self.store.covers_dir().join(&name);
        match std::fs::copy(path, &dest) {
            Ok(_) => {
                let _ = self.dispatch(
                    "item.edit",
                    serde_json::json!({
                        "list_id": list_id,
                        "item_id": item_id,
                        "cover": name.clone(),
                    }),
                );
                self.cover_textures.remove(&name);
                self.persist();
                self.status = "Cover replaced.".to_string();
            }
            Err(e) => {
                self.status = format!("Could not copy cover: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

impl FaveApp {
    fn new_list_dialog(&mut self, ctx: &egui::Context) {
        if !self.new_list.open {
            return;
        }
        let mut open = true;
        egui::Window::new("New list")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut self.new_list.name);
                ui.label("Category");
                egui::ComboBox::from_id_salt("new-list-cat")
                    .selected_text(self.new_list.category.name())
                    .show_ui(ui, |ui| {
                        for cat in Category::ALL {
                            ui.selectable_value(&mut self.new_list.category, cat, cat.name());
                        }
                    });
                ui.checkbox(&mut self.new_list.ranked, "Ranked list");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let can_create = !self.new_list.name.trim().is_empty();
                    if ui
                        .add_enabled(can_create, egui::Button::new("Create list"))
                        .clicked()
                    {
                        let params = serde_json::json!({
                            "name": self.new_list.name.trim(),
                            "category": self.new_list.category.id(),
                            "ranked": self.new_list.ranked,
                        });
                        if let Ok(result) = self.dispatch("list.create", params) {
                            if let Some(id) = result["list_id"].as_u64() {
                                self.selected_list = Some(id);
                            }
                        }
                        self.selected_item = None;
                        self.new_list.open = false;
                        self.new_list.name.clear();
                        self.persist();
                        self.status = "List created.".to_string();
                    }
                    if ui.button("Cancel").clicked() {
                        self.new_list.open = false;
                    }
                });
            });
        if !open {
            self.new_list.open = false;
        }
    }

    fn manual_dialog(&mut self, ctx: &egui::Context) {
        if !self.manual.open {
            return;
        }
        let Some(list) = self.selected_list_ref().cloned() else {
            self.manual.open = false;
            return;
        };
        let mut open = true;
        egui::Window::new("Add manually")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Adding to {} ({})",
                    list.name,
                    list.category.name()
                ));
                ui.label("Title");
                ui.text_edit_singleline(&mut self.manual.title);
                for f in list.category.fields() {
                    ui.label(f.label);
                    let entry = self.manual.fields.entry(f.key.to_string()).or_default();
                    ui.text_edit_singleline(entry);
                }
                ui.horizontal(|ui| {
                    if ui.button("Choose cover image…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
                            .pick_file()
                        {
                            self.manual.cover_path = Some(path);
                        }
                    }
                    if let Some(p) = &self.manual.cover_path {
                        ui.label(egui::RichText::new(p.display().to_string()).small());
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let can_add = !self.manual.title.trim().is_empty();
                    if ui
                        .add_enabled(can_add, egui::Button::new("Add item"))
                        .clicked()
                    {
                        // Field trim/drop-empty normalisation happens in
                        // the registry, same as the detail panel's save.
                        let fields = serde_json::to_value(&self.manual.fields).unwrap_or_default();
                        let mut params = serde_json::json!({
                            "list_id": list.id,
                            "title": self.manual.title.trim(),
                            "fields": fields,
                        });
                        if let Some(cover_path) = self.manual.cover_path.clone() {
                            // A cover that can't be read is skipped, as
                            // before (the item is added without it).
                            if let Ok(bytes) = std::fs::read(&cover_path) {
                                let ext = cover_path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("jpg")
                                    .to_lowercase();
                                params["cover_bytes_base64"] = serde_json::Value::String(
                                    crate::commands::base64_encode(&bytes),
                                );
                                params["cover_ext"] = serde_json::Value::String(ext);
                            }
                        }
                        match self.dispatch("item.add", params) {
                            Ok(r) if r["outcome"] == "duplicate" => {
                                self.status = "That item is already in this list.".to_string();
                            }
                            Ok(_) => {
                                self.status = "Item added.".to_string();
                            }
                            Err(e) => {
                                self.status = format!("Could not add item: {e}");
                            }
                        }
                        self.manual.open = false;
                        self.persist();
                    }
                    if ui.button("Cancel").clicked() {
                        self.manual.open = false;
                    }
                });
            });
        if !open {
            self.manual.open = false;
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_ui.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.heading("API keys");
                ui.label(
                    egui::RichText::new(
                        "Your own free keys. They are stored only in the data folder on this PC.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                ui.label("TMDB API key (v3) — movies & TV shows");
                ui.add(egui::TextEdit::singleline(&mut self.settings_ui.tmdb).password(true));
                ui.label("Twitch Client ID — video games (IGDB)");
                ui.text_edit_singleline(&mut self.settings_ui.igdb_id);
                ui.label("Twitch Client Secret");
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_ui.igdb_secret).password(true),
                );
                ui.separator();
                ui.heading("Data");
                ui.label(
                    egui::RichText::new(format!("Data folder: {}", self.store.data_dir.display()))
                        .small(),
                );
                if ui.button("Move data folder…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        match self.store.relocate(&self.base_dir, &dir) {
                            Ok(()) => {
                                self.cover_textures.clear();
                                self.status =
                                    format!("Data moved to {}.", self.store.data_dir.display());
                            }
                            Err(e) => {
                                self.settings_ui.message =
                                    format!("Could not move data folder: {e}");
                            }
                        }
                    }
                }
                ui.separator();
                ui.heading("Drinks database");
                ui.label(
                    egui::RichText::new(format!(
                        "Bundled pack v{} · {} energy drinks · {} sodas (Open Food Facts, ODbL)",
                        self.drinks.version,
                        self.drinks.count(crate::drinks::DrinkKind::EnergyDrink),
                        self.drinks.count(crate::drinks::DrinkKind::Soda),
                    ))
                    .small(),
                );
                ui.label("Update URL (leave empty until an update server exists)");
                ui.text_edit_singleline(&mut self.settings_ui.drinks_url);
                ui.checkbox(&mut self.settings_ui.auto_update, "Silently auto-update");
                if ui.button("Update drinks database now").clicked() {
                    // Save the URL first so the update uses what's on screen.
                    self.settings.drinks_update_url =
                        self.settings_ui.drinks_url.trim().to_string();
                    self.settings.drinks_auto_update = self.settings_ui.auto_update;
                    self.persist();
                    self.start_drinks_update();
                    self.settings_ui.message = "Checking for a drinks database update…".to_string();
                }
                if !self.settings_ui.message.is_empty() {
                    ui.label(egui::RichText::new(&self.settings_ui.message).small());
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save settings").clicked() {
                        self.settings.tmdb_api_key = self.settings_ui.tmdb.trim().to_string();
                        self.settings.igdb_client_id = self.settings_ui.igdb_id.trim().to_string();
                        self.settings.igdb_client_secret =
                            self.settings_ui.igdb_secret.trim().to_string();
                        self.settings.drinks_update_url =
                            self.settings_ui.drinks_url.trim().to_string();
                        self.settings.drinks_auto_update = self.settings_ui.auto_update;
                        self.persist();
                        self.settings_ui.open = false;
                        self.status = "Settings saved.".to_string();
                    }
                    if ui.button("Cancel").clicked() {
                        self.settings_ui.open = false;
                    }
                });
            });
        if !open {
            self.settings_ui.open = false;
        }
    }

    fn open_text_export(&mut self, list: &FavList) {
        let remembered = self.settings.remembered_export_fields(list.category);
        // Category fields, then the #5 pseudo-fields (rating/note) — the
        // same combined order `export_text::all_field_defs` renders in.
        self.text_export.fields = crate::export_text::all_field_defs(list)
            .iter()
            .map(|f| {
                (
                    f.key.to_string(),
                    f.label.to_string(),
                    remembered.iter().any(|k| k == f.key),
                )
            })
            .collect();
        self.text_export.open = true;
    }

    fn text_export_dialog(&mut self, ctx: &egui::Context) {
        if !self.text_export.open {
            return;
        }
        let Some(list) = self.selected_list_ref().cloned() else {
            self.text_export.open = false;
            return;
        };
        let mut open = true;
        egui::Window::new("Export list as text")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Export “{}”", list.name));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.text_export.markdown, false, "Plain text (.txt)");
                    ui.radio_value(
                        &mut self.text_export.markdown,
                        true,
                        "Markdown (.md, with covers)",
                    );
                });
                ui.label("Include details:");
                for (_, label, checked) in &mut self.text_export.fields {
                    ui.checkbox(checked, label.clone());
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Export…").clicked() {
                        let keys: Vec<String> = self
                            .text_export
                            .fields
                            .iter()
                            .filter(|(_, _, c)| *c)
                            .map(|(k, _, _)| k.clone())
                            .collect();
                        // Remember the choices for next time (spec §7).
                        self.settings
                            .export_fields
                            .insert(list.category.id().to_string(), keys.clone());
                        self.persist();
                        let stem = crate::export_text::safe_file_stem(&list.name);
                        if self.text_export.markdown {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("{stem}.md"))
                                .save_file()
                            {
                                match self.dispatch(
                                    "export.markdown",
                                    serde_json::json!({
                                        "list_id": list.id,
                                        "fields": keys,
                                        "path": path.to_string_lossy().into_owned(),
                                    }),
                                ) {
                                    Ok(_) => {
                                        self.status =
                                            format!("Exported {} (Markdown).", path.display());
                                    }
                                    Err(e) => {
                                        self.status = format!("Export failed: {e}");
                                    }
                                }
                            }
                        } else if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{stem}.txt"))
                            .save_file()
                        {
                            match self.dispatch(
                                "export.text",
                                serde_json::json!({
                                    "list_id": list.id,
                                    "fields": keys,
                                    "path": path.to_string_lossy().into_owned(),
                                }),
                            ) {
                                Ok(_) => {
                                    self.status = format!("Exported {} (text).", path.display());
                                }
                                Err(e) => {
                                    self.status = format!("Export failed: {e}");
                                }
                            }
                        }
                        self.text_export.open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.text_export.open = false;
                    }
                });
            });
        if !open {
            self.text_export.open = false;
        }
    }

    fn open_image_export(&mut self, list: &FavList) {
        self.image_export = ImageExportState {
            open: true,
            list_id: list.id,
            included: vec![true; list.items.len()],
            order: (0..list.items.len()).collect(),
            ..ImageExportState::default()
        };
    }

    fn image_export_dialog(&mut self, ctx: &egui::Context) {
        if !self.image_export.open {
            return;
        }
        let Some(list) = self.library.list(self.image_export.list_id).cloned() else {
            self.image_export.open = false;
            return;
        };
        // Keep included/order in sync with the list.
        if self.image_export.included.len() != list.items.len() {
            self.image_export.included = vec![true; list.items.len()];
            self.image_export.order = (0..list.items.len()).collect();
        }
        let mut open = true;
        let mut do_export = false;
        egui::Window::new("Export list as image")
            .open(&mut open)
            .default_width(680.0)
            .show(ctx, |ui| {
                ui.label("Include:");
                egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                    for (i, item) in list.items.iter().enumerate() {
                        ui.checkbox(&mut self.image_export.included[i], &item.title);
                    }
                });
                let chosen: Vec<usize> = self
                    .image_export
                    .order
                    .iter()
                    .copied()
                    .filter(|&i| self.image_export.included.get(i).copied().unwrap_or(false))
                    .collect();
                ui.horizontal_wrapped(|ui| {
                    ui.label("Arrangement:");
                    egui::ComboBox::from_id_salt("arrangement")
                        .selected_text(self.image_export.arrangement.name())
                        .show_ui(ui, |ui| {
                            for a in Arrangement::ALL {
                                ui.selectable_value(
                                    &mut self.image_export.arrangement,
                                    a,
                                    a.name(),
                                );
                            }
                        });
                    ui.label("Fit:");
                    ui.radio_value(&mut self.image_export.fit, FitMode::CropFill, "Crop to fill");
                    ui.radio_value(&mut self.image_export.fit, FitMode::WholeCover, "Whole cover");
                    ui.label("Format:");
                    ui.radio_value(&mut self.image_export.format, ImageFormat::Png, "PNG");
                    ui.radio_value(&mut self.image_export.format, ImageFormat::Jpeg, "JPG");
                });
                // Missing-cover warning (spec §7): listed BEFORE export.
                let missing: Vec<String> = chosen
                    .iter()
                    .filter(|&&i| list.items[i].cover.is_none())
                    .map(|&i| list.items[i].title.clone())
                    .collect();
                if !missing.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "⚠ No cover for: {} — these will appear as blank tiles. \
                             Add covers from the item details first if you want them pictured.",
                            missing.join(", ")
                        ))
                        .color(COL_ACCENT),
                    );
                }
                ui.label(
                    egui::RichText::new(
                        "Preview — drag one cover onto another to swap them. The exported image contains cover art only, no text.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                self.image_preview_ui(ui, &list, &chosen);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!chosen.is_empty(), egui::Button::new("Export image…"))
                        .clicked()
                    {
                        do_export = true;
                    }
                    if ui.button("Close").clicked() {
                        self.image_export.open = false;
                    }
                });
                if !self.image_export.message.is_empty() {
                    ui.label(egui::RichText::new(&self.image_export.message).small());
                }
            });
        if !open {
            self.image_export.open = false;
        }
        if do_export {
            self.run_image_export(&list);
        }
    }

    fn image_preview_ui(&mut self, ui: &mut egui::Ui, list: &FavList, chosen: &[usize]) {
        if chosen.is_empty() {
            ui.label("Nothing selected.");
            return;
        }
        let avail_w = ui.available_width().min(640.0);
        let (cw, ch) = layout::default_canvas(self.image_export.arrangement, chosen.len());
        let scale = avail_w / cw as f32;
        let preview_h = (ch as f32 * scale).min(460.0);
        let vscale = preview_h / ch as f32;
        let aspects: Vec<f32> = chosen.iter().map(|&i| self.cover_aspect(list, i)).collect();
        let placements = layout::layout(
            self.image_export.arrangement,
            &aspects,
            cw as f32,
            ch as f32,
            14.0,
        );
        let (rect, _resp) = ui.allocate_exact_size(
            egui::vec2(cw as f32 * scale, preview_h),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(0x08, 0x08, 0x08));
        let mut tile_rects: Vec<(usize, egui::Rect)> = Vec::new();
        for (pos, &idx) in chosen.iter().enumerate() {
            let p = placements[pos];
            let r = egui::Rect::from_min_size(
                rect.min + egui::vec2(p.x * scale, p.y * vscale),
                egui::vec2(p.w * scale, p.h * vscale),
            );
            tile_rects.push((pos, r));
            painter.rect_filled(r, 2.0, COL_PANEL);
            if let Some(cover) = &list.items[idx].cover {
                if let Some(tex) = self.cover_texture(ui.ctx(), cover) {
                    painter.image(
                        tex.id(),
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            } else {
                painter.rect_stroke(
                    r,
                    2.0,
                    egui::Stroke::new(1.0f32, COL_DEEP_RED),
                    egui::StrokeKind::Inside,
                );
            }
            let resp = ui.interact(
                r,
                egui::Id::new(("preview-tile", idx)),
                egui::Sense::click_and_drag(),
            );
            if resp.drag_started() {
                self.drag_source = Some(pos);
            }
        }
        if ui.ctx().input(|i| i.pointer.any_released()) {
            if let (Some(from), Some(pos)) = (
                self.drag_source.take(),
                ui.ctx().input(|i| i.pointer.interact_pos()),
            ) {
                if let Some((to, _)) = tile_rects.iter().find(|(_, r)| r.contains(pos)) {
                    if *to != from {
                        // `chosen[k]` is an item index; find where those two
                        // items sit in the full order vector and swap them.
                        let order = &mut self.image_export.order;
                        if let (Some(pf), Some(pt)) = (
                            order.iter().position(|&x| x == chosen[from]),
                            order.iter().position(|&x| x == chosen[*to]),
                        ) {
                            order.swap(pf, pt);
                        }
                    }
                }
            }
        }
    }

    fn cover_aspect(&self, list: &FavList, idx: usize) -> f32 {
        let item = &list.items[idx];
        if let Some(cover) = &item.cover {
            let path = self.store.covers_dir().join(cover);
            if let Ok(img) = image::open(&path) {
                let (w, h) = image::GenericImageView::dimensions(&img);
                if h > 0 {
                    return w as f32 / h as f32;
                }
            }
        }
        // Sensible per-category defaults when no cover is cached.
        match list.category {
            Category::Albums | Category::Podcasts | Category::Artists => 1.0,
            Category::Sodas | Category::EnergyDrinks => 0.55,
            _ => 0.67,
        }
    }

    fn run_image_export(&mut self, list: &FavList) {
        let chosen: Vec<usize> = self
            .image_export
            .order
            .iter()
            .copied()
            .filter(|&i| self.image_export.included.get(i).copied().unwrap_or(false))
            .collect();
        if chosen.is_empty() {
            return;
        }
        // Layout/render/encode now run inside the `export.image` command —
        // the registry loads the same covers and uses the same per-category
        // aspect defaults as this dialog's preview.
        let item_ids: Vec<u64> = chosen.iter().map(|&i| list.items[i].id).collect();
        let ext = self.image_export.format.extension();
        let stem = crate::export_text::safe_file_stem(&list.name);
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        {
            let params = serde_json::json!({
                "list_id": list.id,
                "path": path.to_string_lossy().into_owned(),
                "arrangement": arrangement_id(self.image_export.arrangement),
                "format": match self.image_export.format {
                    ImageFormat::Png => "png",
                    ImageFormat::Jpeg => "jpg",
                },
                "fit": match self.image_export.fit {
                    FitMode::CropFill => "crop_fill",
                    FitMode::WholeCover => "whole_cover",
                },
                "item_ids": item_ids,
            });
            match self.dispatch("export.image", params) {
                Ok(r) => {
                    let missing = r["missing_indices"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    if missing > 0 {
                        // The dialog already warned; exporting with
                        // placeholders is the user's choice at this point.
                        self.image_export.message = format!(
                            "Note: {missing} item(s) have no cover and will be blank tiles."
                        );
                    }
                    self.status = format!("Exported image {}.", path.display());
                    self.image_export.open = false;
                }
                Err(e) => {
                    self.image_export.message = format!("Image export failed: {e}");
                }
            }
        }
    }

    fn backup_dialog(&mut self, ctx: &egui::Context) {
        if !self.backup_ui.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Backup / Restore")
            .open(&mut open)
            .resizable(false)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.heading("Export backup");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.backup_ui.scope_idx, 0, "Single list");
                    ui.radio_value(&mut self.backup_ui.scope_idx, 1, "Selected lists");
                    ui.radio_value(&mut self.backup_ui.scope_idx, 2, "Entire library");
                    ui.radio_value(&mut self.backup_ui.scope_idx, 3, "One category");
                });
                match self.backup_ui.scope_idx {
                    1 => {
                        for (id, checked) in &mut self.backup_ui.selected {
                            if let Some(l) = self.library.list(*id) {
                                let name = l.name.clone();
                                ui.checkbox(checked, name);
                            }
                        }
                    }
                    3 => {
                        egui::ComboBox::from_id_salt("backup-cat")
                            .selected_text(self.backup_ui.category.name())
                            .show_ui(ui, |ui| {
                                for cat in Category::ALL {
                                    ui.selectable_value(
                                        &mut self.backup_ui.category,
                                        cat,
                                        cat.name(),
                                    );
                                }
                            });
                    }
                    _ => {}
                }
                if ui.button("Export backup…").clicked() {
                    let scope = match self.backup_ui.scope_idx {
                        0 => self
                            .selected_list
                            .map(BackupScope::SingleList)
                            .unwrap_or(BackupScope::WholeLibrary),
                        1 => BackupScope::SelectedLists(
                            self.backup_ui
                                .selected
                                .iter()
                                .filter(|(_, c)| *c)
                                .map(|(id, _)| *id)
                                .collect(),
                        ),
                        3 => BackupScope::Category(self.backup_ui.category),
                        _ => BackupScope::WholeLibrary,
                    };
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("favelists-backup.zip")
                        .save_file()
                    {
                        let scope_json = match &scope {
                            BackupScope::SingleList(id) => {
                                serde_json::json!({"kind": "single", "list_id": id})
                            }
                            BackupScope::SelectedLists(ids) => {
                                serde_json::json!({"kind": "selected", "list_ids": ids})
                            }
                            BackupScope::WholeLibrary => serde_json::json!({"kind": "whole"}),
                            BackupScope::Category(c) => {
                                serde_json::json!({"kind": "category", "category": c.id()})
                            }
                        };
                        match self.dispatch(
                            "backup.export",
                            serde_json::json!({
                                "path": path.to_string_lossy().into_owned(),
                                "scope": scope_json,
                            }),
                        ) {
                            Ok(r) => {
                                let n = r["lists_written"].as_u64().unwrap_or(0);
                                self.backup_ui.message =
                                    format!("Exported {n} list(s) to {}.", path.display());
                            }
                            Err(e) => {
                                self.backup_ui.message = format!("Export failed: {e}");
                            }
                        }
                    }
                }
                ui.separator();
                ui.heading("Restore (merge)");
                ui.label(
                    egui::RichText::new(
                        "Importing adds everything from the backup file. Nothing you \
                         already have is deleted or replaced; items already in a list are skipped.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                if ui.button("Import backup…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("FaveLists backup", &["zip"])
                        .pick_file()
                    {
                        match self.dispatch(
                            "backup.import",
                            serde_json::json!({"path": path.to_string_lossy().into_owned()}),
                        ) {
                            Ok(r) => {
                                let count = |k: &str| r[k].as_u64().unwrap_or(0) as usize;
                                let report = ImportReport {
                                    lists_added: count("lists_added"),
                                    lists_merged: count("lists_merged"),
                                    items_added: count("items_added"),
                                    items_skipped_as_duplicates: count(
                                        "items_skipped_as_duplicates",
                                    ),
                                    covers_written: count("covers_written"),
                                };
                                self.cover_textures.clear();
                                self.persist();
                                self.backup_ui.message = import_summary(&report);
                            }
                            Err(e) => {
                                self.backup_ui.message = format!("Import failed: {e}");
                            }
                        }
                    }
                }
                if !self.backup_ui.message.is_empty() {
                    ui.label(egui::RichText::new(&self.backup_ui.message).small());
                }
                if ui.button("Close").clicked() {
                    self.backup_ui.open = false;
                }
            });
        if !open {
            self.backup_ui.open = false;
        }
    }
}

fn import_summary(r: &ImportReport) -> String {
    format!(
        "Import done: {} list(s) added, {} merged, {} item(s) added, {} duplicate(s) skipped, {} cover(s) restored.",
        r.lists_added, r.lists_merged, r.items_added, r.items_skipped_as_duplicates, r.covers_written
    )
}
