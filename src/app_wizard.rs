//! First-run setup wizard (v0.3 program #20) — another
//! `impl FaveApp` block, split out of `app.rs` for the usual reason.
//!
//! The wizard opens ONLY on a genuinely fresh start: `FaveApp::new`
//! found no library, journal, or settings file in the data folder.
//! An existing install is marked `wizard_done` silently at startup
//! and never sees it unasked; Settings can reopen it any time
//! ("Run setup wizard…").
//!
//! Steps: Welcome → API keys → Data folder → Done. The key fields
//! are staged here like the Settings dialog stages its own, and are
//! applied only by Finish — Skip (or closing the window) marks the
//! wizard done and discards anything staged.

use crate::app::{FaveApp, COL_MUTED};
use eframe::egui;

/// Wizard steps, in order.
const STEP_WELCOME: usize = 0;
const STEP_KEYS: usize = 1;
const STEP_DATA: usize = 2;
const STEP_DONE: usize = 3;

#[derive(Default)]
pub(crate) struct WizardState {
    pub open: bool,
    pub step: usize,
    pub tmdb: String,
    pub igdb_id: String,
    pub igdb_secret: String,
    pub message: String,
}

impl WizardState {
    /// (Re)open at the first step with the key fields pre-filled
    /// from the saved settings, so re-running the wizard edits what
    /// is actually stored.
    pub(crate) fn reopen(&mut self, settings: &crate::model::Settings) {
        self.open = true;
        self.step = STEP_WELCOME;
        self.tmdb = settings.tmdb_api_key.clone();
        self.igdb_id = settings.igdb_client_id.clone();
        self.igdb_secret = settings.igdb_client_secret.clone();
        self.message.clear();
    }
}

enum WizardNav {
    None,
    Skip,
    Finish,
}

impl FaveApp {
    pub(crate) fn wizard_dialog(&mut self, ctx: &egui::Context) {
        if !self.wizard.open {
            return;
        }
        let mut open = true;
        let mut nav = WizardNav::None;
        egui::Window::new("Welcome to FaveLists")
            .open(&mut open)
            .resizable(false)
            .default_width(470.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("Setup · step {} of 4", self.wizard.step + 1))
                        .color(COL_MUTED)
                        .small(),
                );
                ui.add_space(4.0);
                match self.wizard.step {
                    STEP_KEYS => self.wizard_keys_step(ui),
                    STEP_DATA => self.wizard_data_step(ui),
                    STEP_DONE => {
                        ui.heading("You're set");
                        ui.label(
                            "That's everything. Create your first list from the sidebar \
                             whenever you're ready — and if you skipped the keys, you can \
                             add them any time in Settings.",
                        );
                    }
                    _ => {
                        ui.heading("Your favorite things, beautifully listed");
                        ui.label(
                            "FaveLists keeps ranked lists of your favorite movies, shows, \
                             books, music, games, podcasts, board games, and drinks. \
                             Everything is stored in a folder on this PC — no account, \
                             nothing leaves the machine except the lookups you ask for.",
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "This short setup covers two optional things: free API \
                                 keys that let the app fill in details and covers for \
                                 you, and where your data folder lives.",
                            )
                            .color(COL_MUTED),
                        );
                    }
                }
                if !self.wizard.message.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&self.wizard.message).small());
                }
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if self.wizard.step < STEP_DONE && ui.button("Skip setup").clicked() {
                        nav = WizardNav::Skip;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.wizard.step == STEP_DONE {
                            if ui.button("Finish").clicked() {
                                nav = WizardNav::Finish;
                            }
                        } else if ui.button("Next →").clicked() {
                            self.wizard.step = (self.wizard.step + 1).min(STEP_DONE);
                            self.wizard.message.clear();
                        }
                        if self.wizard.step > STEP_WELCOME && ui.button("← Back").clicked() {
                            self.wizard.step -= 1;
                            self.wizard.message.clear();
                        }
                    });
                });
            });
        if !open {
            // Closing the window is a skip: done, nothing applied.
            nav = WizardNav::Skip;
        }
        match nav {
            WizardNav::Skip => self.close_wizard(false),
            WizardNav::Finish => self.close_wizard(true),
            WizardNav::None => {}
        }
    }

    /// The API-keys step: the same three fields, with the same
    /// "Get API key" destinations, as the Settings dialog.
    fn wizard_keys_step(&mut self, ui: &mut egui::Ui) {
        ui.heading("API keys (optional)");
        ui.label(
            egui::RichText::new(
                "Your own free keys let the app look up details and covers automatically. \
                 They are stored only in the data folder on this PC, and the app works \
                 without them — drinks use a bundled offline database either way.",
            )
            .color(COL_MUTED)
            .small(),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("TMDB API key (v3) — movies & TV shows");
            ui.hyperlink_to("Get API key ↗", "https://www.themoviedb.org/settings/api");
        });
        ui.add(egui::TextEdit::singleline(&mut self.wizard.tmdb).password(true));
        ui.horizontal(|ui| {
            ui.label("Twitch Client ID — video games (IGDB)");
            ui.hyperlink_to("Get API key ↗", "https://dev.twitch.tv/console/apps");
        });
        ui.text_edit_singleline(&mut self.wizard.igdb_id);
        ui.horizontal(|ui| {
            ui.label("Twitch Client Secret");
            ui.hyperlink_to("Get API key ↗", "https://dev.twitch.tv/console/apps");
        });
        ui.add(egui::TextEdit::singleline(&mut self.wizard.igdb_secret).password(true));
    }

    /// The data-folder step: show where data lives now, and offer
    /// the same relocation the Settings dialog has.
    fn wizard_data_step(&mut self, ui: &mut egui::Ui) {
        ui.heading("Data folder");
        ui.label(
            egui::RichText::new(format!(
                "Your lists, covers, and settings live in:\n{}",
                self.store.data_dir.display()
            ))
            .small(),
        );
        ui.add_space(4.0);
        if ui.button("Move data folder…").clicked() {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                match self.store.relocate(&self.base_dir, &dir) {
                    Ok(()) => {
                        self.cover_textures.clear();
                        self.wizard.message =
                            format!("Data moved to {}.", self.store.data_dir.display());
                    }
                    Err(e) => {
                        self.wizard.message = format!("Could not move data folder: {e}");
                    }
                }
            }
        }
        ui.label(
            egui::RichText::new(
                "Moving is safe: everything in the data folder moves to the new folder, \
                 the old folder is removed, and the app keeps running from the new \
                 location. Nothing is left behind or lost. The default location (next \
                 to the app) is fine for most people.",
            )
            .color(COL_MUTED)
            .small(),
        );
    }

    /// Close the wizard and mark it done. `apply_keys` (Finish, and
    /// only Finish) writes the staged key fields into the settings;
    /// a skip leaves the stored keys untouched.
    fn close_wizard(&mut self, apply_keys: bool) {
        if apply_keys {
            self.settings.tmdb_api_key = self.wizard.tmdb.trim().to_string();
            self.settings.igdb_client_id = self.wizard.igdb_id.trim().to_string();
            self.settings.igdb_client_secret = self.wizard.igdb_secret.trim().to_string();
        }
        self.settings.wizard_done = true;
        self.wizard.open = false;
        self.persist();
        self.status = if apply_keys {
            "Setup complete — welcome to FaveLists.".to_string()
        } else {
            "Setup skipped — you can reopen it any time from Settings.".to_string()
        };
    }
}
