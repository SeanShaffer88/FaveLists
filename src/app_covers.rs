//! Missing-cover sweep (v0.3 program #9) — another `impl FaveApp`
//! block, split out of `app.rs` for the usual reason (that file sits
//! near the publish-size ceiling).
//!
//! The sweep lists every coverless item (the same
//! `views::missing_cover_entries` computation the
//! `library.missing_covers` command serialises, so the view and the
//! registry always agree) and fetches covers for them through the
//! EXISTING cover-fetch path: provider/drinks search by title, the
//! top match's cover (with the details-stage URL for providers whose
//! search carries none, and the artist fallback chain for artists),
//! downloaded and set on the item. In the add flow that chain ends
//! with `item.add`'s `cover_bytes_base64`; here it ends with
//! `item.edit`'s — both write through the registry's shared
//! `write_cover`. All network work runs on background threads and
//! reports back over a channel polled here, like the other dialogs.

use crate::app::{FaveApp, COL_ACCENT, COL_MUTED};
use crate::model::Category;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

/// One cover fetch to run: which item, and what to search for.
#[derive(Clone)]
pub(crate) struct CoverJob {
    list_id: u64,
    item_id: u64,
    category: Category,
    title: String,
}

/// A finished fetch: (list_id, item_id, cover bytes when found).
type CoverOutcome = (u64, u64, Option<Vec<u8>>);

/// Sweep state: open flag, per-item outcome notes ("not found", …),
/// the fetches in flight, the shared result channel, and the
/// Fetch-all queue with its progress.
#[derive(Default)]
pub(crate) struct CoversState {
    pub open: bool,
    pub notes: HashMap<(u64, u64), String>,
    pub in_flight: HashSet<(u64, u64)>,
    pub tx: Option<Sender<CoverOutcome>>,
    pub rx: Option<Receiver<CoverOutcome>>,
    pub fetch_all: bool,
    pub queue: Vec<CoverJob>,
    pub done: usize,
    pub total: usize,
}

/// Fetch cover bytes for one item through the existing search path:
/// drinks items resolve against the bundled database; everything
/// else takes the top provider match — its search-result cover, or
/// the details-stage cover URL for providers whose search has none
/// (board games), or the artist image fallback chain (artists).
/// `None` = no cover found anywhere. Runs on a background thread.
fn fetch_cover_bytes(
    category: Category,
    title: &str,
    settings: &crate::model::Settings,
    drinks: &crate::drinks::DrinksDb,
) -> Option<Vec<u8>> {
    if let Some(kind) = category.drinks_kind() {
        let hit = drinks.search(kind, title, 1).into_iter().next()?;
        return hit
            .image_url
            .as_deref()
            .and_then(|u| crate::providers::download_bytes(u).ok());
    }
    let sug = crate::providers::search(category, title, settings)
        .ok()?
        .into_iter()
        .next()?;
    let cover_url = crate::providers::details_cover_url(category, &sug, settings)
        .or_else(|| sug.cover_url.clone());
    if let Some(bytes) = cover_url
        .as_deref()
        .and_then(|u| crate::providers::download_bytes(u).ok())
    {
        return Some(bytes);
    }
    if category == Category::Artists {
        // MusicBrainz carries no artist photos: the add flow falls
        // back to the key-free Deezer → Wikipedia chain — same here.
        return crate::providers::artist_image::fetch_artist_image(&sug.title);
    }
    None
}

impl FaveApp {
    /// The sweep view (central panel; Esc/Close exits, like the smart
    /// views). Entries are recomputed from the library each frame, so
    /// a row disappears the moment its cover lands.
    pub(crate) fn missing_covers_ui(&mut self, ui: &mut egui::Ui) {
        self.poll_cover_outcomes();
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.covers_ui.open = false;
            return;
        }
        if !self.covers_ui.in_flight.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
        }
        let entries = crate::views::missing_cover_entries(&self.library);
        let mut close = false;
        let mut fetch_all = false;
        let mut stop = false;
        ui.horizontal(|ui| {
            ui.heading("Missing covers");
            ui.label(
                egui::RichText::new(format!("{} without a cover", entries.len())).color(COL_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕ Close").clicked() {
                    close = true;
                }
                if self.covers_ui.fetch_all {
                    if ui.button("Stop").clicked() {
                        stop = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "Fetching {} of {}…",
                            self.covers_ui.done.min(self.covers_ui.total),
                            self.covers_ui.total
                        ))
                        .color(COL_MUTED),
                    );
                } else if !entries.is_empty() && ui.button("Fetch all").clicked() {
                    fetch_all = true;
                }
            });
        });
        ui.separator();
        if close {
            self.covers_ui.open = false;
            return;
        }
        if stop {
            self.covers_ui.fetch_all = false;
            self.covers_ui.queue.clear();
        }
        if fetch_all {
            self.covers_ui.queue = entries
                .iter()
                .map(|e| CoverJob {
                    list_id: e.list_id,
                    item_id: e.item.id,
                    category: e.category,
                    title: e.item.title.clone(),
                })
                .collect();
            self.covers_ui.total = self.covers_ui.queue.len();
            self.covers_ui.done = 0;
            self.covers_ui.fetch_all = true;
            self.spawn_next_cover_fetch();
        }
        if entries.is_empty() {
            ui.label(egui::RichText::new("Every item has a cover. Nice.").color(COL_MUTED));
            return;
        }
        let mut fetch_one: Option<CoverJob> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &entries {
                let key = (entry.list_id, entry.item.id);
                let flying = self.covers_ui.in_flight.contains(&key);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&entry.item.title).strong());
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {}",
                                entry.list_name,
                                entry.category.name()
                            ))
                            .color(COL_MUTED)
                            .small(),
                        );
                        if let Some(note) = self.covers_ui.notes.get(&key) {
                            ui.label(egui::RichText::new(note).color(COL_ACCENT).small());
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if flying {
                            ui.spinner();
                            ui.label("Fetching…");
                        } else if ui.button("Fetch cover").clicked() {
                            fetch_one = Some(CoverJob {
                                list_id: entry.list_id,
                                item_id: entry.item.id,
                                category: entry.category,
                                title: entry.item.title.clone(),
                            });
                        }
                    });
                });
                ui.separator();
            }
        });
        if let Some(job) = fetch_one {
            self.spawn_cover_fetch(job);
        }
    }

    /// Start one fetch on a background thread (no-op when that item is
    /// already being fetched).
    fn spawn_cover_fetch(&mut self, job: CoverJob) {
        let key = (job.list_id, job.item_id);
        if !self.covers_ui.in_flight.insert(key) {
            return;
        }
        if self.covers_ui.tx.is_none() {
            let (tx, rx) = channel();
            self.covers_ui.tx = Some(tx);
            self.covers_ui.rx = Some(rx);
        }
        let tx = self.covers_ui.tx.clone().expect("channel just created");
        let settings = self.settings.clone();
        let drinks = self.drinks.clone();
        std::thread::spawn(move || {
            let bytes = fetch_cover_bytes(job.category, &job.title, &settings, &drinks);
            let _ = tx.send((job.list_id, job.item_id, bytes));
        });
    }

    /// Fetch-all driver: start the next queued job whose item is
    /// still coverless; when the queue runs dry the sweep is done.
    fn spawn_next_cover_fetch(&mut self) {
        while let Some(job) = self.covers_ui.queue.first().cloned() {
            self.covers_ui.queue.remove(0);
            let still_missing = self
                .library
                .list(job.list_id)
                .and_then(|l| l.item(job.item_id))
                .map(|i| i.cover.is_none())
                .unwrap_or(false);
            if still_missing {
                self.spawn_cover_fetch(job);
                return;
            }
            // Landed (or vanished) since the queue was built: counts
            // as done without a fetch.
            self.covers_ui.done += 1;
        }
        self.covers_ui.fetch_all = false;
    }

    /// Drain finished fetches: a found cover is written through
    /// `item.edit`'s cover-bytes params (the add flow's shared
    /// `write_cover` path); a miss leaves the row with a note.
    fn poll_cover_outcomes(&mut self) {
        let mut outcomes: Vec<CoverOutcome> = Vec::new();
        if let Some(rx) = &self.covers_ui.rx {
            while let Ok(outcome) = rx.try_recv() {
                outcomes.push(outcome);
            }
        }
        for (list_id, item_id, bytes) in outcomes {
            let key = (list_id, item_id);
            self.covers_ui.in_flight.remove(&key);
            let title = self
                .library
                .list(list_id)
                .and_then(|l| l.item(item_id))
                .map(|i| i.title.clone())
                .unwrap_or_default();
            match bytes {
                Some(bytes) => {
                    let ext = image::guess_format(&bytes)
                        .map(|f| match f {
                            image::ImageFormat::Png => "png",
                            image::ImageFormat::WebP => "webp",
                            _ => "jpg",
                        })
                        .unwrap_or("jpg");
                    let result = self.dispatch(
                        "item.edit",
                        serde_json::json!({
                            "list_id": list_id,
                            "item_id": item_id,
                            "cover_bytes_base64": crate::commands::base64_encode(&bytes),
                            "cover_ext": ext,
                        }),
                    );
                    match result {
                        Ok(_) => {
                            self.covers_ui.notes.remove(&key);
                            self.status = format!("Cover fetched for {title}.");
                            self.persist();
                        }
                        Err(e) => {
                            self.covers_ui
                                .notes
                                .insert(key, format!("could not save cover: {e}"));
                        }
                    }
                }
                None => {
                    self.covers_ui.notes.insert(key, "not found".to_string());
                }
            }
            if self.covers_ui.fetch_all {
                self.covers_ui.done += 1;
                self.spawn_next_cover_fetch();
            }
        }
    }
}
