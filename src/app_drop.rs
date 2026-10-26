//! Drop-a-cover (v0.3 Slice 5 #17) — another `impl FaveApp` block,
//! split out of `app.rs` like the other feature modules.
//!
//! Dragging an image file onto a cover tile (or a row, in rows
//! view) sets that item's cover; dropping anywhere else in the
//! window falls back to the currently selected item; with neither,
//! the status line says an item must be selected or hovered. The
//! set itself is not new machinery: the file's bytes go through the
//! existing `item.edit` cover-bytes params (v0.3 Slice 4) — the
//! same `write_cover` path the missing-cover sweep uses — and the
//! item's cached texture is dropped so the new cover shows at once.
//!
//! Paste: egui delivers pasted *text* only, and no clipboard crate
//! is in the tree, so a pasted image cannot reach the app. What
//! does work: pasting the *path* of an image file while an item is
//! selected (and no text field has focus) sets that item's cover
//! from the file. The gap is recorded in `docs/PARITY.md`.

use crate::app::FaveApp;
use eframe::egui;
use std::path::{Path, PathBuf};

/// The image extensions a dropped/pasted cover may use. `jpeg`
/// normalises to `jpg`, matching the cover cache's naming.
pub fn cover_ext_for_path(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => Some("jpg"),
        Some("png") => Some("png"),
        Some("webp") => Some("webp"),
        _ => None,
    }
}

/// Classify pasted text as a cover-file path: the trimmed text must
/// be a single line naming a file with an accepted image extension.
/// Existence is the caller's check (kept out of this pure helper).
pub fn pasted_cover_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains(['\n', '\r']) {
        return None;
    }
    let path = PathBuf::from(trimmed);
    cover_ext_for_path(&path)?;
    Some(path)
}

/// Which item a drop point lands on: the first recorded target rect
/// containing the point. Targets are (item_id, rect) pairs recorded
/// by the grid / rows rendering of the current list each frame.
pub fn target_at(targets: &[(u64, egui::Rect)], pos: egui::Pos2) -> Option<u64> {
    targets
        .iter()
        .find(|(_, rect)| rect.contains(pos))
        .map(|(id, _)| *id)
}

impl FaveApp {
    /// Process this frame's file drops and path pastes. Called from
    /// `update` after the central panel has rendered, so the drop
    /// targets recorded by the grid/rows are this frame's.
    pub(crate) fn handle_cover_drop(&mut self, ctx: &egui::Context) {
        // -- drag-and-drop -------------------------------------------------
        let dropped: Vec<Option<PathBuf>> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            let hover_pos = ctx.input(|i| i.pointer.hover_pos());
            let hovered = hover_pos.and_then(|pos| target_at(&self.drop_targets, pos));
            for path in dropped.into_iter().flatten() {
                // A drop not over a tile/row lands on the selected
                // item — but only when that item is in the list the
                // recorded targets belong to.
                let target = hovered.or_else(|| {
                    self.selected_item
                        .filter(|id| self.drop_targets.iter().any(|(t, _)| t == id))
                });
                match target {
                    Some(item_id) => {
                        if let Some(list_id) = self.drop_list {
                            self.set_cover_from_file(list_id, item_id, &path);
                        }
                    }
                    None => {
                        self.status =
                            "Drop a cover onto an item, or select an item first.".to_string();
                    }
                }
            }
            return;
        }
        // -- pasted image-file path ---------------------------------------
        // A focused text field owns paste (it is text input); only an
        // unfocused paste can mean "use this path as a cover".
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let pastes: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Paste(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        });
        for text in pastes {
            let Some(path) = pasted_cover_path(&text) else {
                continue;
            };
            if !path.is_file() {
                continue;
            }
            if let (Some(list_id), Some(item_id)) = (self.selected_list, self.selected_item) {
                let in_list = self
                    .library
                    .list(list_id)
                    .map(|l| l.item(item_id).is_some())
                    .unwrap_or(false);
                if in_list {
                    self.set_cover_from_file(list_id, item_id, &path);
                }
            }
        }
    }

    /// Set one item's cover from an image file on disk, through the
    /// registry's cover-bytes path. Every failure lands in the
    /// status line; nothing is half-applied (the command validates
    /// before it mutates).
    fn set_cover_from_file(&mut self, list_id: u64, item_id: u64, path: &Path) {
        let Some(ext) = cover_ext_for_path(path) else {
            self.status = format!(
                "{} isn't a JPG, PNG or WebP image — cover unchanged.",
                path.display()
            );
            return;
        };
        let title = self
            .library
            .list(list_id)
            .and_then(|l| l.item(item_id))
            .map(|i| i.title.clone())
            .unwrap_or_default();
        let old_cover = self
            .library
            .list(list_id)
            .and_then(|l| l.item(item_id))
            .and_then(|i| i.cover.clone());
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.status = format!("Could not read {}: {e}", path.display());
                return;
            }
        };
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
                // The cover file is (re)written under the item's
                // stable name; drop any cached texture for the old
                // and new names so the fresh bytes render.
                if let Some(old) = &old_cover {
                    self.cover_textures.remove(old);
                }
                self.cover_textures
                    .remove(&format!("cover-{item_id}.{ext}"));
                self.persist();
                self.status = format!("Cover set for {title}.");
            }
            Err(e) => {
                self.status = format!("Could not set cover: {e}");
            }
        }
    }
}
