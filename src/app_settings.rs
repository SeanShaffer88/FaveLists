//! Settings dialog (API keys, data folder, drinks database) — a fourth
//! `impl FaveApp` block, split out of `app.rs` for the same reason as
//! `app_bulk.rs` / `app_share.rs`: that file is near the publish-size
//! ceiling, and the v0.2.1 settings changes (Get API key links, the
//! move-data safety note) needed the room.
//!
//! Like the rest of the GUI, the dialog only renders state and collects
//! intent: key fields edit the staged `SettingsState`, the data-folder
//! move goes through `Store::relocate`, and saving writes through
//! `persist()`.

use crate::app::{import_summary, FaveApp, COL_MUTED};
use crate::backup::ImportReport;
use eframe::egui;

impl FaveApp {
    pub(crate) fn settings_dialog(&mut self, ctx: &egui::Context) {
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
                ui.horizontal(|ui| {
                    ui.label("TMDB API key (v3) — movies & TV shows");
                    ui.hyperlink_to(
                        "Get API key ↗",
                        "https://www.themoviedb.org/settings/api",
                    );
                });
                ui.add(egui::TextEdit::singleline(&mut self.settings_ui.tmdb).password(true));
                ui.horizontal(|ui| {
                    ui.label("Twitch Client ID — video games (IGDB)");
                    ui.hyperlink_to("Get API key ↗", "https://dev.twitch.tv/console/apps");
                });
                ui.text_edit_singleline(&mut self.settings_ui.igdb_id);
                ui.horizontal(|ui| {
                    ui.label("Twitch Client Secret");
                    ui.hyperlink_to("Get API key ↗", "https://dev.twitch.tv/console/apps");
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_ui.igdb_secret).password(true),
                );
                ui.separator();
                ui.heading("Appearance");
                ui.label(
                    egui::RichText::new(
                        "These apply immediately and are remembered — no need to save.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                // Text size (v0.3 #14): applies the zoom right away
                // and saves, like every appearance control here.
                ui.horizontal(|ui| {
                    ui.label("Text size");
                    for size in crate::appearance::TextSize::ALL {
                        if ui
                            .selectable_label(self.settings.text_size == size, size.label())
                            .clicked()
                            && self.settings.text_size != size
                        {
                            self.settings.text_size = size;
                            ctx.set_zoom_factor(size.factor());
                            self.persist();
                        }
                    }
                });
                // Cover size (v0.3 #15): the grid's tile scale.
                let scale_resp = ui.add(
                    egui::Slider::new(
                        &mut self.settings.cover_scale,
                        crate::appearance::COVER_SCALE_MIN..=crate::appearance::COVER_SCALE_MAX,
                    )
                    .text("Cover size"),
                );
                if scale_resp.changed() {
                    self.persist();
                }
                // Rows view (v0.3 #15): the same setting the list
                // header's "Rows" toggle flips.
                if ui
                    .checkbox(&mut self.settings.rows_view, "Rows view (compact list)")
                    .changed()
                {
                    self.persist();
                }
                // High contrast (v0.3 #16): visuals re-install every
                // frame from this flag, so the toggle is immediate.
                if ui
                    .checkbox(&mut self.settings.high_contrast, "High contrast")
                    .changed()
                {
                    ctx.set_visuals(crate::appearance::visuals(self.settings.high_contrast));
                    self.persist();
                }
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
                ui.label(
                    egui::RichText::new(
                        "Moving is safe: everything in the data folder — your lists, covers, settings and drinks database — moves to the new folder, the old folder is removed, and the app keeps running from the new location. Nothing is left behind or lost.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                ui.separator();
                ui.heading("Backups");
                // Automatic backups (v0.3 #19): a whole-library ZIP
                // once a week, the newest four kept, written by the
                // same machinery as the manual backup export. The
                // toggle applies immediately, like Appearance.
                if ui
                    .checkbox(&mut self.settings.auto_backups, "Automatic weekly backups")
                    .changed()
                {
                    self.persist();
                }
                ui.label(
                    egui::RichText::new(
                        "Kept in a backups folder inside the data folder; the newest four are kept.",
                    )
                    .color(COL_MUTED)
                    .small(),
                );
                if ui.button("Back up now").clicked() {
                    self.spawn_backup(true);
                    self.settings_ui.message = "Backing up…".to_string();
                }
                let auto_backups =
                    crate::auto_backup::list_auto_backups(&self.store.backups_dir());
                if auto_backups.is_empty() {
                    ui.label(
                        egui::RichText::new("No automatic backups yet.")
                            .color(COL_MUTED)
                            .small(),
                    );
                } else {
                    // Newest first. Restoring goes through the
                    // existing merge import — nothing is replaced.
                    let mut restore: Option<std::path::PathBuf> = None;
                    for (name, mtime) in auto_backups.iter().rev() {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{name} · {}",
                                    crate::model::iso_date(*mtime)
                                ))
                                .small(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Restore").clicked() {
                                        restore = Some(self.store.backups_dir().join(name));
                                    }
                                },
                            );
                        });
                    }
                    if let Some(path) = restore {
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
                                self.settings_ui.message = import_summary(&report);
                            }
                            Err(e) => {
                                self.settings_ui.message = format!("Restore failed: {e}");
                            }
                        }
                    }
                }
                // Update check (v0.3 #18): quiet, launch-time only,
                // and silent on any failure.
                if ui
                    .checkbox(
                        &mut self.settings.check_for_updates,
                        "Check for a newer version on launch",
                    )
                    .changed()
                {
                    self.persist();
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
                if ui.button("Run setup wizard…").clicked() {
                    self.wizard.reopen(&self.settings);
                    self.settings_ui.open = false;
                }
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
}
