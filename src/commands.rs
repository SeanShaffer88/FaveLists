//! Command registry (v0.2 program #1 — the Storytold foundation slice).
//!
//! Every core library operation is registered here under a stable name with
//! a handler that takes structured (serde JSON) params and returns a
//! structured result or a [`CommandError`]. The same dispatch is used by:
//!
//! * the GUI (`app.rs` routes its core call sites through [`dispatch`]),
//! * the headless JSON channel (`json_channel.rs`, `--json-channel`),
//! * tests, which can therefore prove both surfaces behave identically.
//!
//! No GUI types appear here. Handlers mutate the [`Library`] in the
//! [`CommandContext`]; persisting is the caller's job (the GUI calls its
//! existing `persist()`, the JSON channel saves after mutating commands).
//!
//! Later program slices register here too: Slice 3 (#3 + #9) added the
//! bulk item commands, `library.find_duplicates` and
//! `library.merge_duplicates`; #5 rating/note extends `item.add`/
//! `item.edit` params; Slice 5 (#8 + #10) added `export.share_card` and
//! `import.csv`. The registry is the frozen interface those slices
//! build on.
//!
//! Slice 2 (#4 + #2) added undo/redo and filtered global search:
//! [`History`] is the session-layer undo stack. When a [`CommandContext`]
//! carries a history handle, [`dispatch`] snapshots the library before
//! every successful *mutating* command (deep clone; capped at
//! [`HISTORY_CAP`] entries; a command that leaves the library unchanged —
//! e.g. a duplicate `item.add` — records nothing, and a new mutation
//! clears the redo stack). `history.undo` / `history.redo` are ordinary
//! registered commands that swap the library with those snapshots, so the
//! GUI and the JSON channel undo identically. Snapshots capture the whole
//! [`Library`], so undo restores item ids, order, ranks, the list set, and
//! the id counters exactly. (Cover *files* written by a command are not
//! un-written; undo only restores library state, leaving at worst an
//! orphaned file in the cover cache.)

use crate::backup::{self, BackupScope};
use crate::csv_import::{self, ImportError, ImportTarget};
use crate::duplicates;
use crate::export_text;
use crate::layout::{self, Arrangement};
use crate::model::{Category, FavList, Item, Library, SortKind};
use crate::render::{self, FitMode, ImageFormat};
use crate::share_card;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A structured command failure. `code` is stable and machine-readable:
/// `unknown_command`, `bad_params`, `not_found`, `io`, or `internal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    pub fn bad_params(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "bad_params",
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "not_found",
            message: message.into(),
        }
    }

    pub fn io(e: std::io::Error) -> CommandError {
        CommandError {
            code: "io",
            message: e.to_string(),
        }
    }

    pub fn internal(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "internal",
            message: message.into(),
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

// ---------------------------------------------------------------------------
// Context + registry
// ---------------------------------------------------------------------------

/// Everything a handler may touch: the library being operated on and the
/// cover cache directory (exports and cover writes resolve against it).
/// `history` is the optional session undo stack (Slice 2 #4): surfaces
/// that want undo (the GUI, the JSON channel) attach one via
/// [`CommandContext::with_history`]; plain contexts dispatch without
/// recording history, and `history.undo`/`history.redo` then report a
/// clean no-op result.
pub struct CommandContext<'a> {
    pub library: &'a mut Library,
    pub covers_dir: &'a Path,
    pub history: Option<&'a mut History>,
}

impl<'a> CommandContext<'a> {
    pub fn new(library: &'a mut Library, covers_dir: &'a Path) -> CommandContext<'a> {
        CommandContext {
            library,
            covers_dir,
            history: None,
        }
    }

    pub fn with_history(
        library: &'a mut Library,
        covers_dir: &'a Path,
        history: &'a mut History,
    ) -> CommandContext<'a> {
        CommandContext {
            library,
            covers_dir,
            history: Some(history),
        }
    }
}

// ---------------------------------------------------------------------------
// Undo history (Slice 2 #4)
// ---------------------------------------------------------------------------

/// Maximum number of undo steps retained; older snapshots are dropped.
pub const HISTORY_CAP: usize = 50;

/// Session undo/redo stacks. Each entry is `(command name, library
/// snapshot)`: on the undo stack the snapshot is the state *before* that
/// command ran; on the redo stack it is the state *before* the undo that
/// displaced it. Owned by the surface (GUI app, JSON channel session) and
/// lent to each dispatch through [`CommandContext`], so the registry
/// itself stays free of GUI types and both surfaces share semantics.
#[derive(Debug, Default)]
pub struct History {
    undo_stack: Vec<(String, Library)>,
    redo_stack: Vec<(String, Library)>,
}

impl History {
    pub fn new() -> History {
        History::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// Record the pre-command snapshot for a successful mutating command.
    /// A fresh mutation invalidates everything that was undone.
    fn record(&mut self, command: &str, before: Library) {
        self.undo_stack.push((command.to_string(), before));
        trim_stack(&mut self.undo_stack);
        self.redo_stack.clear();
    }
}

fn trim_stack(stack: &mut Vec<(String, Library)>) {
    while stack.len() > HISTORY_CAP {
        stack.remove(0);
    }
}

type Handler = fn(&mut CommandContext<'_>, &Value) -> Result<Value, CommandError>;

/// One registered command.
pub struct CommandSpec {
    pub name: &'static str,
    pub summary: &'static str,
    /// Mutating commands change the library; the JSON channel persists
    /// after them (and only them).
    pub mutating: bool,
    handler: Handler,
}

/// The registry. Names are the frozen interface — GUI, JSON channel, and
/// future surfaces all dispatch through this table.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "commands.list",
        summary: "List every registered command (name, summary, mutating).",
        mutating: false,
        handler: cmd_commands_list,
    },
    CommandSpec {
        name: "library.get",
        summary: "Return the whole library as JSON.",
        mutating: false,
        handler: cmd_library_get,
    },
    CommandSpec {
        name: "library.stats",
        summary: "Return {lists, items} counts for the library.",
        mutating: false,
        handler: cmd_library_stats,
    },
    CommandSpec {
        name: "list.create",
        summary: "Create a list. Params: name, category (category id), ranked.",
        mutating: true,
        handler: cmd_list_create,
    },
    CommandSpec {
        name: "list.get",
        summary: "Return one list as JSON. Params: list_id.",
        mutating: false,
        handler: cmd_list_get,
    },
    CommandSpec {
        name: "list.rename",
        summary: "Rename a list. Params: list_id, name.",
        mutating: true,
        handler: cmd_list_rename,
    },
    CommandSpec {
        name: "list.delete",
        summary: "Delete a list. Params: list_id.",
        mutating: true,
        handler: cmd_list_delete,
    },
    CommandSpec {
        name: "list.set_ranked",
        summary: "Switch a list between ranked and unranked. Params: list_id, ranked.",
        mutating: true,
        handler: cmd_list_set_ranked,
    },
    CommandSpec {
        name: "list.sort",
        summary: "One-shot sort. Params: list_id, kind (title|year|added|rating). Rating: highest first, unrated last.",
        mutating: true,
        handler: cmd_list_sort,
    },
    CommandSpec {
        name: "list.query",
        summary: "In-list filter: items whose title contains the query (case-insensitive). Params: list_id, query.",
        mutating: false,
        handler: cmd_list_query,
    },
    CommandSpec {
        name: "library.search",
        summary: "The list.query predicate applied across every list, with optional filters. Params: query (title contains, case-insensitive; empty matches every title), category? (category id), year? (the item's year/years field contains the text), genre? (genre field contains, case-insensitive).",
        mutating: false,
        handler: cmd_library_search,
    },
    CommandSpec {
        name: "library.find_duplicates",
        summary: "Find groups of the same work appearing in 2+ places (non-mutating). Result: {groups: [{category, key, confidence (exact|possible), copies: [{list_id, list_name, item_id, title, year}]}]}. Matching: category + normalised title (leading article stripped); exact when years agree or are missing on some copies, possible when years conflict.",
        mutating: false,
        handler: cmd_library_find_duplicates,
    },
    CommandSpec {
        name: "library.merge_duplicates",
        summary: "Merge duplicate copies into one kept copy. Params: keep {list_id, item_id}, drop [{list_id, item_id}, ...] (at least one). The kept item first adopts any field it lacks (and a cover / provider id) from the dropped copies, then the dropped copies are removed. One dispatch = one undo step.",
        mutating: true,
        handler: cmd_library_merge_duplicates,
    },
    CommandSpec {
        name: "history.undo",
        summary: "Undo the last mutating command (restores the exact prior library state). Result: {undone, command?}.",
        mutating: true,
        handler: cmd_history_undo,
    },
    CommandSpec {
        name: "history.redo",
        summary: "Redo the last undone command. Result: {redone, command?}.",
        mutating: true,
        handler: cmd_history_redo,
    },
    CommandSpec {
        name: "item.add",
        summary: "Add an item to a list (same-work duplicates are reported, not added). Params: list_id, title, fields?, provider_id?, source?, cover?, cover_bytes_base64?+cover_ext?, added_at?, rating? (1-10), note?.",
        mutating: true,
        handler: cmd_item_add,
    },
    CommandSpec {
        name: "item.remove",
        summary: "Remove an item from a list. Params: list_id, item_id.",
        mutating: true,
        handler: cmd_item_remove,
    },
    CommandSpec {
        name: "item.edit",
        summary: "Edit an item: title?, fields? (wholesale replace), cover? (name, or null to clear), rating? (1-10, or null to clear), note? (trimmed; empty clears). Params: list_id, item_id.",
        mutating: true,
        handler: cmd_item_edit,
    },
    CommandSpec {
        name: "item.move",
        summary: "Move the item at index `from` to index `to`. Params: list_id, from, to.",
        mutating: true,
        handler: cmd_item_move,
    },
    CommandSpec {
        name: "item.swap",
        summary: "Swap the items at two indices. Params: list_id, a, b.",
        mutating: true,
        handler: cmd_item_swap,
    },
    CommandSpec {
        name: "item.set_rank",
        summary: "Move an item to a 1-based rank position. Params: list_id, item_id, rank.",
        mutating: true,
        handler: cmd_item_set_rank,
    },
    CommandSpec {
        name: "items.bulk_remove",
        summary: "Remove several items from one list in a single dispatch (one undo step). Params: list_id, item_ids[]. Result lists the ids actually removed; ids not in the list are ignored.",
        mutating: true,
        handler: cmd_items_bulk_remove,
    },
    CommandSpec {
        name: "items.bulk_move",
        summary: "Move items to another list, preserving item ids and relative order (appended in source order). Params: from_list_id, to_list_id, item_ids[]. Items whose work is already in the target are skipped and reported; moving within the same list is a no-op. One dispatch = one undo step.",
        mutating: true,
        handler: cmd_items_bulk_move,
    },
    CommandSpec {
        name: "items.bulk_copy",
        summary: "Copy items to another list with NEW item ids (the cover file reference is shared, not duplicated). Params: from_list_id, to_list_id, item_ids[]. Items whose work is already in the target are skipped and reported. One dispatch = one undo step.",
        mutating: true,
        handler: cmd_items_bulk_copy,
    },
    CommandSpec {
        name: "export.text",
        summary: "Write a .txt export. Params: list_id, path, fields? (field keys; default all).",
        mutating: false,
        handler: cmd_export_text,
    },
    CommandSpec {
        name: "export.markdown",
        summary: "Write a .md export with cover assets. Params: list_id, path, fields?.",
        mutating: false,
        handler: cmd_export_markdown,
    },
    CommandSpec {
        name: "export.image",
        summary: "Render a cover-art image export (no text). Params: list_id, path, arrangement?, format?, fit?, item_ids?.",
        mutating: false,
        handler: cmd_export_image,
    },
    CommandSpec {
        name: "export.share_card",
        summary: "Render a designed share-card PNG (1200x630, WITH text: list name header, rank/cover/title/year rows). Params: list_id, path, count? (default 10, clamped 1-25). Requires a system font (loaded at runtime, never bundled); result reports entries rendered and missing_indices for coverless items.",
        mutating: false,
        handler: cmd_export_share_card,
    },
    CommandSpec {
        name: "backup.export",
        summary: "Write a backup ZIP. Params: path, scope {kind: single|selected|whole|category, list_id?, list_ids?, category?}.",
        mutating: false,
        handler: cmd_backup_export,
    },
    CommandSpec {
        name: "backup.import",
        summary: "Merge-import a backup ZIP (never deletes or replaces). Params: path.",
        mutating: true,
        handler: cmd_backup_import,
    },
    CommandSpec {
        name: "import.csv",
        summary: "Merge-import a diary/ratings/library CSV export (format detected by header signature; never overwrites). Params: path, target {mode: new, name, category?} | {mode: existing, list_id}. One dispatch = one undo step. Result: {format, added, skipped, skipped_duplicates, skipped_wrong_category, invalid, lists: [{list_id, name, category, added}]}.",
        mutating: true,
        handler: cmd_import_csv,
    },
];

/// Look up a registered command by name.
pub fn spec(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// All registered command names, in registry order.
pub fn command_names() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.name).collect()
}

/// Whether a command mutates the library (unknown names: false — dispatch
/// will reject them anyway).
pub fn is_mutating(name: &str) -> bool {
    spec(name).map(|c| c.mutating).unwrap_or(false)
}

/// The two commands that manage the history stacks themselves; the
/// generic snapshot logic in [`dispatch`] must not record them.
fn is_history_command(name: &str) -> bool {
    matches!(name, "history.undo" | "history.redo")
}

/// Dispatch one command by name with structured params.
///
/// When the context carries a [`History`], a successful mutating command
/// first snapshots the library; the snapshot is recorded for undo only if
/// the command actually changed the library (a duplicate `item.add`, say,
/// leaves no undo step). Failed commands record nothing.
pub fn dispatch(
    ctx: &mut CommandContext<'_>,
    name: &str,
    params: &Value,
) -> Result<Value, CommandError> {
    let spec = match spec(name) {
        Some(spec) => spec,
        None => {
            return Err(CommandError {
                code: "unknown_command",
                message: format!("no command named '{name}'"),
            })
        }
    };
    let track = spec.mutating && !is_history_command(name) && ctx.history.is_some();
    let before = if track {
        Some(ctx.library.clone())
    } else {
        None
    };
    let result = (spec.handler)(ctx, params);
    if let (Ok(_), Some(before)) = (&result, before) {
        if *ctx.library != before {
            if let Some(history) = ctx.history.as_deref_mut() {
                history.record(name, before);
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Param helpers — every failure is a clean `bad_params`, never a panic.
// ---------------------------------------------------------------------------

fn obj(params: &Value) -> Result<&Map<String, Value>, CommandError> {
    params
        .as_object()
        .ok_or_else(|| CommandError::bad_params("params must be a JSON object"))
}

fn req_u64(m: &Map<String, Value>, key: &str) -> Result<u64, CommandError> {
    m.get(key).and_then(Value::as_u64).ok_or_else(|| {
        CommandError::bad_params(format!(
            "missing or invalid '{key}' (expected an unsigned integer)"
        ))
    })
}

fn opt_u64(m: &Map<String, Value>, key: &str) -> Option<u64> {
    m.get(key).and_then(Value::as_u64)
}

fn req_str<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a str, CommandError> {
    m.get(key).and_then(Value::as_str).ok_or_else(|| {
        CommandError::bad_params(format!("missing or invalid '{key}' (expected a string)"))
    })
}

fn opt_str(m: &Map<String, Value>, key: &str) -> Option<String> {
    m.get(key).and_then(Value::as_str).map(|s| s.to_string())
}

fn req_bool(m: &Map<String, Value>, key: &str) -> Result<bool, CommandError> {
    m.get(key).and_then(Value::as_bool).ok_or_else(|| {
        CommandError::bad_params(format!("missing or invalid '{key}' (expected a boolean)"))
    })
}

fn opt_bool(m: &Map<String, Value>, key: &str, default: bool) -> bool {
    m.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn req_usize(m: &Map<String, Value>, key: &str) -> Result<usize, CommandError> {
    req_u64(m, key).map(|v| v as usize)
}

/// Parse an `item_ids` array param (unsigned integers). Ids that do not
/// resolve are the caller's concern — the bulk commands ignore them and
/// report what actually happened.
fn req_id_array(m: &Map<String, Value>, key: &str) -> Result<Vec<u64>, CommandError> {
    let arr = m.get(key).and_then(Value::as_array).ok_or_else(|| {
        CommandError::bad_params(format!("missing or invalid '{key}' (expected an array)"))
    })?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        out.push(v.as_u64().ok_or_else(|| {
            CommandError::bad_params(format!("'{key}' entries must be unsigned integers"))
        })?);
    }
    Ok(out)
}

/// Parse one `{list_id, item_id}` object (the merge command's `keep` and
/// `drop` entries).
fn parse_item_ref(v: &Value) -> Result<(u64, u64), CommandError> {
    let o = v.as_object().ok_or_else(|| {
        CommandError::bad_params("item reference must be an object {list_id, item_id}")
    })?;
    Ok((req_u64(o, "list_id")?, req_u64(o, "item_id")?))
}

fn parse_category(m: &Map<String, Value>, key: &str) -> Result<Category, CommandError> {
    let id = req_str(m, key)?;
    Category::from_id(id)
        .ok_or_else(|| CommandError::bad_params(format!("unknown category id '{id}'")))
}

/// Parse an optional `fields` object (string -> string). Empty/whitespace
/// values are dropped and values trimmed — the same normalisation the GUI
/// applies before saving an item.
fn parse_fields(m: &Map<String, Value>) -> Result<Option<BTreeMap<String, String>>, CommandError> {
    match m.get("fields") {
        None => Ok(None),
        Some(Value::Object(o)) => {
            let mut out = BTreeMap::new();
            for (k, v) in o {
                let s = v.as_str().ok_or_else(|| {
                    CommandError::bad_params(format!("fields.'{k}' must be a string"))
                })?;
                let t = s.trim();
                if !t.is_empty() {
                    out.insert(k.clone(), t.to_string());
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(CommandError::bad_params("'fields' must be an object")),
    }
}

/// Parse the optional `rating` param (v0.2 #5): absent → `Ok(None)` (leave
/// the item's rating alone); JSON null → `Ok(Some(None))` (clear); an
/// integer 1–10 → `Ok(Some(Some(n)))`. Anything else — out of range,
/// fractional, a string — is a clean `bad_params` and the caller must not
/// have mutated anything yet.
fn parse_rating(m: &Map<String, Value>) -> Result<Option<Option<u8>>, CommandError> {
    match m.get("rating") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                CommandError::bad_params("'rating' must be an integer 1-10 or null")
            })?;
            if !(1..=10).contains(&n) {
                return Err(CommandError::bad_params(
                    "'rating' must be between 1 and 10",
                ));
            }
            Ok(Some(Some(n as u8)))
        }
    }
}

/// Parse the optional `note` param (v0.2 #5): absent → `Ok(None)`; a
/// string → `Ok(Some(trimmed))` (empty after trimming clears the note);
/// anything else is a clean `bad_params`.
fn parse_note(m: &Map<String, Value>) -> Result<Option<String>, CommandError> {
    match m.get("note") {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_string())),
        Some(_) => Err(CommandError::bad_params("'note' must be a string")),
    }
}

/// Field keys for a text/Markdown export: the `fields` param when given
/// (category keys plus the #5 pseudo-fields "rating"/"note"), otherwise
/// all of the list category's fields.
fn export_field_keys(m: &Map<String, Value>, list: &FavList) -> Result<Vec<String>, CommandError> {
    match m.get("fields") {
        None => Ok(list
            .category
            .fields()
            .iter()
            .map(|f| f.key.to_string())
            .collect()),
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for v in a {
                let s = v
                    .as_str()
                    .ok_or_else(|| CommandError::bad_params("'fields' entries must be strings"))?;
                out.push(s.to_string());
            }
            Ok(out)
        }
        Some(_) => Err(CommandError::bad_params(
            "'fields' must be an array of field keys",
        )),
    }
}

fn list_ref<'a>(ctx: &'a CommandContext<'_>, id: u64) -> Result<&'a FavList, CommandError> {
    ctx.library
        .list(id)
        .ok_or_else(|| CommandError::not_found(format!("no list with id {id}")))
}

fn list_mut<'a>(ctx: &'a mut CommandContext<'_>, id: u64) -> Result<&'a mut FavList, CommandError> {
    ctx.library
        .list_mut(id)
        .ok_or_else(|| CommandError::not_found(format!("no list with id {id}")))
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, CommandError> {
    serde_json::to_value(v).map_err(|e| CommandError::internal(format!("serialise result: {e}")))
}

// ---------------------------------------------------------------------------
// Handlers — meta / library
// ---------------------------------------------------------------------------

fn cmd_commands_list(
    _ctx: &mut CommandContext<'_>,
    _params: &Value,
) -> Result<Value, CommandError> {
    let list: Vec<Value> = COMMANDS
        .iter()
        .map(|c| json!({"name": c.name, "summary": c.summary, "mutating": c.mutating}))
        .collect();
    Ok(json!({"commands": list}))
}

fn cmd_library_get(ctx: &mut CommandContext<'_>, _params: &Value) -> Result<Value, CommandError> {
    to_json(&*ctx.library)
}

fn cmd_library_stats(ctx: &mut CommandContext<'_>, _params: &Value) -> Result<Value, CommandError> {
    Ok(json!({
        "lists": ctx.library.lists.len(),
        "items": ctx.library.total_items(),
    }))
}

fn cmd_list_create(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let name = req_str(m, "name")?.trim().to_string();
    if name.is_empty() {
        return Err(CommandError::bad_params("'name' must not be empty"));
    }
    let category = parse_category(m, "category")?;
    let ranked = opt_bool(m, "ranked", false);
    let id = ctx.library.new_list(name, category, ranked);
    let list = list_ref(ctx, id)?;
    Ok(json!({"list_id": id, "list": to_json(list)?}))
}

fn cmd_list_get(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list = list_ref(ctx, req_u64(m, "list_id")?)?;
    to_json(list)
}

fn cmd_list_rename(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let id = req_u64(m, "list_id")?;
    let name = req_str(m, "name")?.to_string();
    list_mut(ctx, id)?.name = name.clone();
    Ok(json!({"list_id": id, "name": name}))
}

fn cmd_list_delete(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let id = req_u64(m, "list_id")?;
    if ctx.library.remove_list(id) {
        Ok(json!({"list_id": id, "deleted": true}))
    } else {
        Err(CommandError::not_found(format!("no list with id {id}")))
    }
}

fn cmd_list_set_ranked(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let id = req_u64(m, "list_id")?;
    let ranked = req_bool(m, "ranked")?;
    list_mut(ctx, id)?.set_ranked(ranked);
    Ok(json!({"list_id": id, "ranked": ranked}))
}

fn cmd_list_sort(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let id = req_u64(m, "list_id")?;
    let kind = match req_str(m, "kind")? {
        "title" => SortKind::TitleAZ,
        "year" => SortKind::Year,
        "added" => SortKind::DateAdded,
        "rating" => SortKind::Rating,
        other => {
            return Err(CommandError::bad_params(format!(
                "unknown sort kind '{other}' (expected title|year|added|rating)"
            )))
        }
    };
    list_mut(ctx, id)?.sort(kind);
    Ok(json!({"list_id": id, "sorted": true}))
}

/// The in-list filter predicate, exactly as the GUI's grid applies it:
/// case-insensitive substring on the title; empty query matches everything.
fn title_matches(title: &str, query_lower: &str) -> bool {
    query_lower.is_empty() || title.to_lowercase().contains(query_lower)
}

fn cmd_list_query(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list = list_ref(ctx, req_u64(m, "list_id")?)?;
    let q = opt_str(m, "query")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let items: Vec<Value> = list
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| title_matches(&it.title, &q))
        .map(|(i, it)| Ok(json!({"index": i, "item": to_json(it)?})))
        .collect::<Result<_, CommandError>>()?;
    Ok(json!({"list_id": list.id, "matches": items}))
}

/// An optional string param that is a clean `bad_params` when present but
/// not a string (silently ignoring a mistyped filter would hide bugs).
fn opt_str_strict(m: &Map<String, Value>, key: &str) -> Result<Option<String>, CommandError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(CommandError::bad_params(format!(
            "'{key}' must be a string"
        ))),
    }
}

fn cmd_library_search(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    // Empty/absent query matches every title (same predicate as
    // `list.query`); the filters below then do the narrowing.
    let q = opt_str(m, "query")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let category = match m.get("category") {
        None | Some(Value::Null) => None,
        Some(_) => Some(parse_category(m, "category")?),
    };
    let year = opt_str_strict(m, "year")?
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let genre = opt_str_strict(m, "genre")?
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let mut matches = Vec::new();
    for list in &ctx.library.lists {
        if let Some(cat) = category {
            if list.category != cat {
                continue;
            }
        }
        for (i, it) in list.items.iter().enumerate() {
            if !title_matches(&it.title, &q) {
                continue;
            }
            if let Some(y) = &year {
                // The category's year field ("year", or "years" for TV):
                // the given text must appear in it ("197" finds 1979,
                // "2020" finds the range "2017–2020"). Items in categories
                // without a year field never match a year filter.
                let hit = list
                    .category
                    .year_key()
                    .and_then(|k| it.field(k))
                    .map(|v| v.to_lowercase().contains(y.as_str()))
                    .unwrap_or(false);
                if !hit {
                    continue;
                }
            }
            if let Some(g) = &genre {
                let hit = it
                    .field("genre")
                    .map(|v| v.to_lowercase().contains(g.as_str()))
                    .unwrap_or(false);
                if !hit {
                    continue;
                }
            }
            matches.push(json!({
                "list_id": list.id,
                "list_name": list.name,
                "category": list.category.id(),
                "index": i,
                "item_id": it.id,
                "title": it.title,
                "item": to_json(it)?,
            }));
        }
    }
    Ok(json!({"matches": matches}))
}

// ---------------------------------------------------------------------------
// Handlers — duplicate finder + merge (Slice 3 #3)
// ---------------------------------------------------------------------------

fn cmd_library_find_duplicates(
    ctx: &mut CommandContext<'_>,
    _params: &Value,
) -> Result<Value, CommandError> {
    let groups = duplicates::find_duplicates(ctx.library);
    Ok(json!({
        "groups": groups.iter().map(duplicates::DupGroup::to_json).collect::<Vec<_>>(),
    }))
}

fn cmd_library_merge_duplicates(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let keep_ref = m
        .get("keep")
        .ok_or_else(|| CommandError::bad_params("missing 'keep' {list_id, item_id}"))?;
    let (keep_list_id, keep_item_id) = parse_item_ref(keep_ref)?;
    let drop_arr = m.get("drop").and_then(Value::as_array).ok_or_else(|| {
        CommandError::bad_params(
            "missing or invalid 'drop' (expected an array of {list_id, item_id})",
        )
    })?;
    if drop_arr.is_empty() {
        return Err(CommandError::bad_params(
            "'drop' must name at least one copy to remove",
        ));
    }
    let mut drops: Vec<(u64, u64)> = Vec::with_capacity(drop_arr.len());
    for v in drop_arr {
        let r = parse_item_ref(v)?;
        if !drops.contains(&r) {
            drops.push(r);
        }
    }
    if drops.contains(&(keep_list_id, keep_item_id)) {
        return Err(CommandError::bad_params(
            "the kept copy must not also appear in 'drop'",
        ));
    }
    // Validate EVERYTHING before mutating: every named list and item must
    // exist, so an invalid merge errors with zero state change (and, via
    // dispatch, records no undo step).
    let mut kept = list_ref(ctx, keep_list_id)?
        .item(keep_item_id)
        .cloned()
        .ok_or_else(|| {
            CommandError::not_found(format!(
                "no item with id {keep_item_id} in list {keep_list_id}"
            ))
        })?;
    let mut dropped_items: Vec<Item> = Vec::with_capacity(drops.len());
    for &(l, i) in &drops {
        let item = list_ref(ctx, l)?
            .item(i)
            .cloned()
            .ok_or_else(|| CommandError::not_found(format!("no item with id {i} in list {l}")))?;
        dropped_items.push(item);
    }
    // Adoption: the kept copy takes anything it lacks from the dropped
    // copies — detail fields first-non-empty-wins in drop order, a cover
    // if it has none, and a provider identity if it has none (so future
    // duplicate checks see the most complete record). Fields the kept
    // copy already has are never overwritten.
    let mut adopted_fields: Vec<String> = Vec::new();
    let mut adopted_cover = false;
    let mut adopted_provider = false;
    for dropped in &dropped_items {
        for (key, value) in &dropped.fields {
            if kept.field(key).is_none() && !value.trim().is_empty() {
                kept.fields.insert(key.clone(), value.clone());
                adopted_fields.push(key.clone());
            }
        }
        if kept.cover.is_none() && dropped.cover.is_some() {
            kept.cover = dropped.cover.clone();
            adopted_cover = true;
        }
        let kept_has_pid = kept
            .provider_id
            .as_deref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false);
        let dropped_has_pid = dropped
            .provider_id
            .as_deref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false);
        if !kept_has_pid && dropped_has_pid {
            kept.provider_id = dropped.provider_id.clone();
            kept.source = dropped.source.clone();
            adopted_provider = true;
        }
    }
    // Write the enriched kept copy back, then remove the dropped copies.
    // (Existence was validated above and nothing has mutated since, so
    // these lookups cannot fail; they still return structured errors
    // rather than panicking, per registry style.)
    let keep_list = list_mut(ctx, keep_list_id)?;
    let slot = keep_list.item_mut(keep_item_id).ok_or_else(|| {
        CommandError::not_found(format!(
            "no item with id {keep_item_id} in list {keep_list_id}"
        ))
    })?;
    *slot = kept;
    let mut removed: Vec<Value> = Vec::with_capacity(drops.len());
    for &(l, i) in &drops {
        list_mut(ctx, l)?.remove_item(i);
        removed.push(json!({"list_id": l, "item_id": i}));
    }
    Ok(json!({
        "kept": {"list_id": keep_list_id, "item_id": keep_item_id},
        "removed": removed,
        "adopted_fields": adopted_fields,
        "adopted_cover": adopted_cover,
        "adopted_provider": adopted_provider,
    }))
}

// ---------------------------------------------------------------------------
// Handlers — history (Slice 2 #4)
// ---------------------------------------------------------------------------

fn cmd_history_undo(ctx: &mut CommandContext<'_>, _params: &Value) -> Result<Value, CommandError> {
    let CommandContext {
        library, history, ..
    } = ctx;
    let Some(history) = history.as_deref_mut() else {
        // No history attached to this session: a clean no-op, not an
        // error — the command itself is well-formed.
        return Ok(json!({"undone": false, "command": Value::Null}));
    };
    match history.undo_stack.pop() {
        Some((command, snapshot)) => {
            let current = std::mem::replace(*library, snapshot);
            history.redo_stack.push((command.clone(), current));
            trim_stack(&mut history.redo_stack);
            Ok(json!({
                "undone": true,
                "command": command,
                "can_undo": history.can_undo(),
                "can_redo": history.can_redo(),
            }))
        }
        None => Ok(json!({"undone": false, "command": Value::Null})),
    }
}

fn cmd_history_redo(ctx: &mut CommandContext<'_>, _params: &Value) -> Result<Value, CommandError> {
    let CommandContext {
        library, history, ..
    } = ctx;
    let Some(history) = history.as_deref_mut() else {
        return Ok(json!({"redone": false, "command": Value::Null}));
    };
    match history.redo_stack.pop() {
        Some((command, snapshot)) => {
            let current = std::mem::replace(*library, snapshot);
            history.undo_stack.push((command.clone(), current));
            trim_stack(&mut history.undo_stack);
            Ok(json!({
                "redone": true,
                "command": command,
                "can_undo": history.can_undo(),
                "can_redo": history.can_redo(),
            }))
        }
        None => Ok(json!({"redone": false, "command": Value::Null})),
    }
}

// ---------------------------------------------------------------------------
// Handlers — items
// ---------------------------------------------------------------------------

fn cmd_item_add(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let title = req_str(m, "title")?.trim().to_string();
    if title.is_empty() {
        return Err(CommandError::bad_params("'title' must not be empty"));
    }
    let fields = parse_fields(m)?.unwrap_or_default();
    let rating = parse_rating(m)?.flatten();
    let note = parse_note(m)?.unwrap_or_default();
    // Validate the target list exists before allocating an id or writing a
    // cover, so a bad list_id leaves no trace.
    list_ref(ctx, list_id)?;
    let item_id = ctx.library.alloc_item_id();
    let mut item = Item::new(item_id, title);
    item.fields = fields;
    item.rating = rating;
    item.note = note;
    item.provider_id = opt_str(m, "provider_id");
    if let Some(source) = opt_str(m, "source") {
        item.source = source;
    }
    if let Some(added) = opt_u64(m, "added_at") {
        item.added_at = added as i64;
    }
    if let Some(cover) = opt_str(m, "cover") {
        item.cover = Some(cover);
    }
    if let Some(b64) = opt_str(m, "cover_bytes_base64") {
        let ext = opt_str(m, "cover_ext").unwrap_or_else(|| "jpg".to_string());
        item.cover = Some(write_cover(ctx.covers_dir, item_id, &b64, &ext)?);
    }
    let list = list_mut(ctx, list_id)?;
    match list.add_item(item) {
        crate::model::AddOutcome::Added => {
            Ok(json!({"list_id": list_id, "item_id": item_id, "outcome": "added"}))
        }
        crate::model::AddOutcome::Duplicate => {
            Ok(json!({"list_id": list_id, "item_id": item_id, "outcome": "duplicate"}))
        }
    }
}

fn cmd_item_remove(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let item_id = req_u64(m, "item_id")?;
    let list = list_mut(ctx, list_id)?;
    if list.remove_item(item_id) {
        Ok(json!({"list_id": list_id, "item_id": item_id, "removed": true}))
    } else {
        Err(CommandError::not_found(format!(
            "no item with id {item_id} in list {list_id}"
        )))
    }
}

fn cmd_item_edit(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let item_id = req_u64(m, "item_id")?;
    let new_title = opt_str(m, "title");
    let new_fields = parse_fields(m)?;
    // Rating/note are validated BEFORE the item is looked up and mutated,
    // so an invalid rating errors with zero state change.
    let new_rating = parse_rating(m)?;
    let new_note = parse_note(m)?;
    let cover_present = m.contains_key("cover");
    let new_cover: Option<Option<String>> = if cover_present {
        match m.get("cover") {
            Some(Value::String(s)) => Some(Some(s.clone())),
            Some(Value::Null) => Some(None),
            _ => return Err(CommandError::bad_params("'cover' must be a string or null")),
        }
    } else {
        None
    };
    let list = list_mut(ctx, list_id)?;
    let item = list.item_mut(item_id).ok_or_else(|| {
        CommandError::not_found(format!("no item with id {item_id} in list {list_id}"))
    })?;
    if let Some(t) = new_title {
        item.title = t;
    }
    if let Some(f) = new_fields {
        item.fields = f;
    }
    if let Some(r) = new_rating {
        item.rating = r;
    }
    if let Some(n) = new_note {
        item.note = n;
    }
    if let Some(c) = new_cover {
        item.cover = c;
    }
    Ok(json!({"list_id": list_id, "item_id": item_id, "item": to_json(item)?}))
}

fn cmd_item_move(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let from = req_usize(m, "from")?;
    let to = req_usize(m, "to")?;
    list_mut(ctx, list_id)?.move_item(from, to);
    Ok(json!({"list_id": list_id, "moved": true}))
}

fn cmd_item_swap(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let a = req_usize(m, "a")?;
    let b = req_usize(m, "b")?;
    list_mut(ctx, list_id)?.swap_items(a, b);
    Ok(json!({"list_id": list_id, "swapped": true}))
}

fn cmd_item_set_rank(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let item_id = req_u64(m, "item_id")?;
    let rank = req_u64(m, "rank")?;
    if rank == 0 {
        return Err(CommandError::bad_params("'rank' is 1-based (must be >= 1)"));
    }
    let list = list_mut(ctx, list_id)?;
    let from = list
        .items
        .iter()
        .position(|i| i.id == item_id)
        .ok_or_else(|| {
            CommandError::not_found(format!("no item with id {item_id} in list {list_id}"))
        })?;
    let to = ((rank - 1) as usize).min(list.items.len() - 1);
    list.move_item(from, to);
    Ok(json!({"list_id": list_id, "item_id": item_id, "rank": to + 1}))
}

// ---------------------------------------------------------------------------
// Handlers — bulk item actions (Slice 3 #9)
//
// Each bulk command is a single dispatch, so Slice 2's snapshot logic
// makes it exactly one undo step. Lists are validated before any
// mutation; unknown item ids are ignored and the result reports what
// actually happened. Move/copy use the model's existing same-work
// definition (`FavList::contains_work` — provider id, else normalised
// title), the same rule `item.add` enforces.
// ---------------------------------------------------------------------------

/// The requested items of `list_id` that are actually present, in the
/// list's own order (not request order), cloned for processing.
fn present_items_in_order(
    ctx: &CommandContext<'_>,
    list_id: u64,
    ids: &[u64],
) -> Result<Vec<Item>, CommandError> {
    let list = list_ref(ctx, list_id)?;
    Ok(list
        .items
        .iter()
        .filter(|it| ids.contains(&it.id))
        .cloned()
        .collect())
}

fn cmd_items_bulk_remove(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list_id = req_u64(m, "list_id")?;
    let ids = req_id_array(m, "item_ids")?;
    let list = list_mut(ctx, list_id)?;
    let mut removed: Vec<u64> = Vec::new();
    for id in &ids {
        if list.remove_item(*id) {
            removed.push(*id);
        }
    }
    Ok(json!({"list_id": list_id, "removed": removed}))
}

fn cmd_items_bulk_move(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let from_id = req_u64(m, "from_list_id")?;
    let to_id = req_u64(m, "to_list_id")?;
    let ids = req_id_array(m, "item_ids")?;
    // Both lists must exist before anything moves.
    list_ref(ctx, from_id)?;
    list_ref(ctx, to_id)?;
    if from_id == to_id {
        return Ok(json!({
            "from_list_id": from_id,
            "to_list_id": to_id,
            "moved": [],
            "skipped": [],
            "noop": true,
        }));
    }
    let moving = present_items_in_order(ctx, from_id, &ids)?;
    let mut moved: Vec<u64> = Vec::new();
    let mut skipped: Vec<u64> = Vec::new();
    for item in moving {
        // Checked against the target as it grows, so two requested
        // copies of the same work move only once.
        let duplicate = list_ref(ctx, to_id)?.contains_work(&item);
        if duplicate {
            skipped.push(item.id);
            continue;
        }
        let id = item.id;
        list_mut(ctx, from_id)?.remove_item(id);
        list_mut(ctx, to_id)?.items.push(item);
        moved.push(id);
    }
    Ok(json!({
        "from_list_id": from_id,
        "to_list_id": to_id,
        "moved": moved,
        "skipped": skipped,
    }))
}

fn cmd_items_bulk_copy(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let from_id = req_u64(m, "from_list_id")?;
    let to_id = req_u64(m, "to_list_id")?;
    let ids = req_id_array(m, "item_ids")?;
    list_ref(ctx, from_id)?;
    list_ref(ctx, to_id)?;
    let candidates = present_items_in_order(ctx, from_id, &ids)?;
    let mut copied: Vec<Value> = Vec::new();
    let mut skipped: Vec<u64> = Vec::new();
    for item in candidates {
        // Also covers from == to: the target already holds the work (the
        // item itself), so every copy is skipped.
        let duplicate = list_ref(ctx, to_id)?.contains_work(&item);
        if duplicate {
            skipped.push(item.id);
            continue;
        }
        let source_id = item.id;
        let new_id = ctx.library.alloc_item_id();
        let mut copy = item;
        copy.id = new_id;
        // The cover *file* is shared by reference — the copy points at
        // the same cached file name; no bytes are duplicated.
        list_mut(ctx, to_id)?.items.push(copy);
        copied.push(json!({"source_item_id": source_id, "item_id": new_id}));
    }
    Ok(json!({
        "from_list_id": from_id,
        "to_list_id": to_id,
        "copied": copied,
        "skipped": skipped,
    }))
}

// ---------------------------------------------------------------------------
// Handlers — exports
// ---------------------------------------------------------------------------

fn cmd_export_text(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list = list_ref(ctx, req_u64(m, "list_id")?)?.clone();
    let keys = export_field_keys(m, &list)?;
    let path = req_str(m, "path")?;
    export_text::write_txt(&list, &keys, Path::new(path)).map_err(CommandError::io)?;
    Ok(json!({"path": path, "written": true}))
}

fn cmd_export_markdown(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list = list_ref(ctx, req_u64(m, "list_id")?)?.clone();
    let keys = export_field_keys(m, &list)?;
    let path = req_str(m, "path")?;
    let covers_dir = ctx.covers_dir.to_path_buf();
    let written = export_text::write_md(&list, &keys, Path::new(path), &covers_dir)
        .map_err(CommandError::io)?;
    Ok(json!({"path": path, "written": true, "assets_written": written.len()}))
}

fn parse_arrangement(s: &str) -> Result<Arrangement, CommandError> {
    Ok(match s {
        "uniform_grid" => Arrangement::UniformGrid,
        "topster" => Arrangement::Topster,
        "mosaic" => Arrangement::Mosaic,
        "shelf_rows" => Arrangement::ShelfRows,
        "scattered" => Arrangement::Scattered,
        "spiral" => Arrangement::Spiral,
        "honeycomb" => Arrangement::Honeycomb,
        "ring" => Arrangement::Ring,
        "filmstrip" => Arrangement::Filmstrip,
        "masonry" => Arrangement::Masonry,
        other => {
            return Err(CommandError::bad_params(format!(
                "unknown arrangement '{other}'"
            )))
        }
    })
}

/// Fallback cover aspect when no cover file can be read — mirrors the GUI's
/// per-category defaults in `app::FaveApp::cover_aspect`.
fn default_aspect(category: Category) -> f32 {
    match category {
        Category::Albums | Category::Podcasts | Category::Artists => 1.0,
        Category::Sodas | Category::EnergyDrinks => 0.55,
        _ => 0.67,
    }
}

fn cmd_export_image(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list = list_ref(ctx, req_u64(m, "list_id")?)?.clone();
    let path = req_str(m, "path")?.to_string();
    let arrangement = parse_arrangement(
        &opt_str(m, "arrangement").unwrap_or_else(|| "uniform_grid".to_string()),
    )?;
    let format = match opt_str(m, "format").as_deref() {
        None | Some("png") => ImageFormat::Png,
        Some("jpg") | Some("jpeg") => ImageFormat::Jpeg,
        Some(other) => {
            return Err(CommandError::bad_params(format!(
                "unknown image format '{other}' (expected png|jpg)"
            )))
        }
    };
    let fit = match opt_str(m, "fit").as_deref() {
        None | Some("crop_fill") => FitMode::CropFill,
        Some("whole_cover") => FitMode::WholeCover,
        Some(other) => {
            return Err(CommandError::bad_params(format!(
                "unknown fit '{other}' (expected crop_fill|whole_cover)"
            )))
        }
    };
    // Which items, in export order: explicit item_ids, else the whole list.
    let chosen: Vec<&Item> = match m.get("item_ids") {
        None => list.items.iter().collect(),
        Some(Value::Array(ids)) => {
            let mut out = Vec::new();
            for v in ids {
                let id = v.as_u64().ok_or_else(|| {
                    CommandError::bad_params("'item_ids' entries must be unsigned integers")
                })?;
                let item = list.item(id).ok_or_else(|| {
                    CommandError::not_found(format!("no item with id {id} in list {}", list.id))
                })?;
                out.push(item);
            }
            out
        }
        Some(_) => return Err(CommandError::bad_params("'item_ids' must be an array")),
    };
    if chosen.is_empty() {
        return Err(CommandError::bad_params(
            "no items selected for image export",
        ));
    }
    let covers_dir = ctx.covers_dir;
    let images: Vec<Option<image::DynamicImage>> = chosen
        .iter()
        .map(|it| {
            it.cover
                .as_ref()
                .and_then(|c| image::open(covers_dir.join(c)).ok())
        })
        .collect();
    // Aspect per item: from the actual cover file when it loads (the GUI
    // does the same), else the per-category default.
    let aspects: Vec<f32> = chosen
        .iter()
        .map(|it| {
            let from_file = it.cover.as_ref().and_then(|c| {
                image::open(covers_dir.join(c)).ok().and_then(|im| {
                    let (w, h) = image::GenericImageView::dimensions(&im);
                    if h > 0 {
                        Some(w as f32 / h as f32)
                    } else {
                        None
                    }
                })
            });
            from_file.unwrap_or_else(|| default_aspect(list.category))
        })
        .collect();
    let missing = render::missing_covers(&images.iter().map(|o| o.as_ref()).collect::<Vec<_>>());
    let (cw, ch) = layout::default_canvas(arrangement, chosen.len());
    let placements = layout::layout(arrangement, &aspects, cw as f32, ch as f32, 14.0);
    let covers_ref: Vec<Option<&image::DynamicImage>> = images.iter().map(|o| o.as_ref()).collect();
    let img = render::render(&placements, &covers_ref, cw, ch, arrangement, fit);
    let bytes = render::encode(&img, format).map_err(CommandError::internal)?;
    std::fs::write(&path, bytes).map_err(CommandError::io)?;
    Ok(json!({
        "path": path,
        "written": true,
        "width": cw,
        "height": ch,
        "missing_indices": missing,
    }))
}

// ---------------------------------------------------------------------------
// Handlers — backup
// ---------------------------------------------------------------------------

fn parse_scope(m: &Map<String, Value>) -> Result<BackupScope, CommandError> {
    let scope = m.get("scope").and_then(Value::as_object).ok_or_else(|| {
        CommandError::bad_params("missing or invalid 'scope' (expected an object)")
    })?;
    match req_str(scope, "kind")? {
        "single" => Ok(BackupScope::SingleList(req_u64(scope, "list_id")?)),
        "selected" => {
            let ids = scope
                .get("list_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    CommandError::bad_params("scope 'selected' needs 'list_ids' (array)")
                })?;
            let mut out = Vec::new();
            for v in ids {
                out.push(v.as_u64().ok_or_else(|| {
                    CommandError::bad_params("'list_ids' entries must be unsigned integers")
                })?);
            }
            Ok(BackupScope::SelectedLists(out))
        }
        "whole" => Ok(BackupScope::WholeLibrary),
        "category" => Ok(BackupScope::Category(parse_category(scope, "category")?)),
        other => Err(CommandError::bad_params(format!(
            "unknown backup scope kind '{other}' (expected single|selected|whole|category)"
        ))),
    }
}

fn cmd_backup_export(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let scope = parse_scope(m)?;
    let path = req_str(m, "path")?;
    let covers_dir = ctx.covers_dir.to_path_buf();
    let n = backup::export_backup(&*ctx.library, &scope, &covers_dir, Path::new(path))
        .map_err(CommandError::io)?;
    Ok(json!({"path": path, "lists_written": n}))
}

fn cmd_backup_import(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let path = req_str(m, "path")?;
    let covers_dir = ctx.covers_dir.to_path_buf();
    let report = backup::import_backup(&mut *ctx.library, &covers_dir, Path::new(path))
        .map_err(CommandError::io)?;
    Ok(json!({
        "path": path,
        "lists_added": report.lists_added,
        "lists_merged": report.lists_merged,
        "items_added": report.items_added,
        "items_skipped_as_duplicates": report.items_skipped_as_duplicates,
        "covers_written": report.covers_written,
    }))
}

// ---------------------------------------------------------------------------
// Handlers — share card (Slice 5 #8) + CSV import (Slice 5 #10)
// ---------------------------------------------------------------------------

fn cmd_export_share_card(
    ctx: &mut CommandContext<'_>,
    params: &Value,
) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let list = list_ref(ctx, req_u64(m, "list_id")?)?.clone();
    let path = req_str(m, "path")?.to_string();
    // count: default 10, clamped to the card's 1-25 range.
    let count = match m.get("count") {
        None | Some(Value::Null) => 10,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| CommandError::bad_params("'count' must be an unsigned integer"))?
            as usize,
    }
    .clamp(1, share_card::MAX_ENTRIES);
    if list.items.is_empty() {
        return Err(CommandError::bad_params(
            "cannot make a share card for an empty list",
        ));
    }
    let chosen: Vec<&Item> = list.items.iter().take(count).collect();
    let covers_dir = ctx.covers_dir;
    let images: Vec<Option<image::DynamicImage>> = chosen
        .iter()
        .map(|it| {
            it.cover
                .as_ref()
                .and_then(|c| image::open(covers_dir.join(c)).ok())
        })
        .collect();
    let entries: Vec<share_card::CardEntry<'_>> = chosen
        .iter()
        .zip(images.iter())
        .map(|(it, cover)| share_card::CardEntry {
            title: &it.title,
            year: list
                .category
                .year_key()
                .and_then(|k| it.field(k))
                .filter(|y| !y.trim().is_empty()),
            cover: cover.as_ref(),
        })
        .collect();
    // A missing system font is a clean io-style error, not a panic and
    // not a silently textless card (see share_card's module docs).
    let rendered =
        share_card::render_card(&list.name, &entries).map_err(|message| CommandError {
            code: "io",
            message,
        })?;
    let bytes =
        render::encode(&rendered.image, ImageFormat::Png).map_err(CommandError::internal)?;
    std::fs::write(&path, bytes).map_err(CommandError::io)?;
    Ok(json!({
        "path": path,
        "written": true,
        "width": share_card::CARD_W,
        "height": share_card::CARD_H,
        "entries": entries.len(),
        "missing_indices": rendered.missing_indices,
    }))
}

/// Parse the `import.csv` `target` param into the engine's target type.
fn parse_import_target(m: &Map<String, Value>) -> Result<ImportTarget, CommandError> {
    let t = m.get("target").and_then(Value::as_object).ok_or_else(|| {
        CommandError::bad_params("missing or invalid 'target' (expected an object)")
    })?;
    match req_str(t, "mode")? {
        "new" => {
            let name = req_str(t, "name")?.trim().to_string();
            if name.is_empty() {
                return Err(CommandError::bad_params("'target.name' must not be empty"));
            }
            let category = match t.get("category") {
                None | Some(Value::Null) => None,
                Some(_) => Some(parse_category(t, "category")?),
            };
            Ok(ImportTarget::New { name, category })
        }
        "existing" => Ok(ImportTarget::Existing {
            list_id: req_u64(t, "list_id")?,
        }),
        other => Err(CommandError::bad_params(format!(
            "unknown target mode '{other}' (expected new|existing)"
        ))),
    }
}

fn cmd_import_csv(ctx: &mut CommandContext<'_>, params: &Value) -> Result<Value, CommandError> {
    let m = obj(params)?;
    let path = req_str(m, "path")?;
    let target = parse_import_target(m)?;
    let text = std::fs::read_to_string(path).map_err(|e| {
        CommandError::io(std::io::Error::new(e.kind(), format!("read {path}: {e}")))
    })?;
    let outcome =
        csv_import::import_csv_text(&mut *ctx.library, &text, &target).map_err(|e| match e {
            ImportError::ListNotFound(id) => {
                CommandError::not_found(format!("no list with id {id}"))
            }
            ImportError::Parse(msg) | ImportError::CategoryMismatch(msg) => {
                CommandError::bad_params(msg)
            }
        })?;
    let format = outcome
        .format
        .map(|f| f.id().to_string())
        .unwrap_or_default();
    Ok(json!({
        "path": path,
        "format": format,
        "added": outcome.added,
        "skipped": outcome.skipped(),
        "skipped_duplicates": outcome.skipped_duplicates,
        "skipped_wrong_category": outcome.skipped_wrong_category,
        "invalid": outcome.invalid,
        "lists": outcome.lists.iter().map(|l| json!({
            "list_id": l.list_id,
            "name": l.name,
            "category": l.category.id(),
            "added": l.added,
        })).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------
// Base64 (RFC 4648, standard alphabet) — hand-rolled so the registry needs
// no new dependency. Used for cover bytes in `item.add` params.
// ---------------------------------------------------------------------------

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as base64 (with padding).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode base64 (padding optional, whitespace ignored). `None` on any
/// invalid character or length.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut vals = Vec::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_whitespace() || ch == '=' {
            continue;
        }
        let v = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 26,
            '0'..='9' => ch as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return None,
        };
        vals.push(v);
    }
    if vals.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(vals.len() * 3 / 4);
    for chunk in vals.chunks(4) {
        let mut n: u32 = 0;
        for (i, v) in chunk.iter().enumerate() {
            n |= v << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Write cover bytes into the cover cache under the item's stable cover
/// file name; returns the file name stored on the item.
fn write_cover(
    covers_dir: &Path,
    item_id: u64,
    b64: &str,
    ext_hint: &str,
) -> Result<String, CommandError> {
    let bytes = base64_decode(b64)
        .ok_or_else(|| CommandError::bad_params("'cover_bytes_base64' is not valid base64"))?;
    let name = crate::store::cover_file_name(item_id, &format!("cover.{ext_hint}"));
    std::fs::create_dir_all(covers_dir).map_err(CommandError::io)?;
    std::fs::write(covers_dir.join(&name), bytes).map_err(CommandError::io)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip_and_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"\xff\x00\xfe"), "/wD+");
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVsbG8").unwrap(), b"hello"); // padding optional
        assert_eq!(base64_decode("/wD+").unwrap(), vec![0xff, 0x00, 0xfe]);
        assert!(base64_decode("not*base64").is_none());
        let data: Vec<u8> = (0..=255u8).collect();
        assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data);
    }

    #[test]
    fn unknown_command_is_a_clean_error() {
        let mut lib = Library::default();
        let mut ctx = CommandContext::new(&mut lib, Path::new("."));
        let err = dispatch(&mut ctx, "nope.nope", &json!({})).unwrap_err();
        assert_eq!(err.code, "unknown_command");
    }

    #[test]
    fn commands_list_includes_the_core_set() {
        let names = command_names();
        for expected in [
            "list.create",
            "list.rename",
            "list.delete",
            "item.add",
            "item.remove",
            "item.edit",
            "item.move",
            "item.set_rank",
            "list.query",
            "export.text",
            "export.markdown",
            "export.image",
            "backup.export",
            "backup.import",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn history_undo_redo_round_trips_exact_state() {
        let mut lib = Library::default();
        let mut history = History::new();
        let covers = Path::new(".");
        let mut ctx = CommandContext::with_history(&mut lib, covers, &mut history);
        let before = ctx.library.clone();
        dispatch(
            &mut ctx,
            "list.create",
            &json!({"name": "Films", "category": "movies", "ranked": true}),
        )
        .unwrap();
        dispatch(
            &mut ctx,
            "item.add",
            &json!({"list_id": 1, "title": "Alien", "fields": {"year": "1979"}}),
        )
        .unwrap();
        dispatch(
            &mut ctx,
            "item.move",
            &json!({"list_id": 1, "from": 0, "to": 0}),
        )
        .unwrap(); // move to self: no state change, so no undo step
        let after_adds = ctx.library.clone();
        let r = dispatch(&mut ctx, "history.undo", &json!({})).unwrap();
        assert_eq!(r["undone"], true);
        assert_eq!(r["command"], "item.add");
        let r = dispatch(&mut ctx, "history.undo", &json!({})).unwrap();
        assert_eq!(r["command"], "list.create");
        assert_eq!(*ctx.library, before); // ids AND counters restored
        assert_eq!(ctx.library.next_list_id, 1);
        assert_eq!(ctx.library.next_item_id, 1);
        let r = dispatch(&mut ctx, "history.redo", &json!({})).unwrap();
        assert_eq!(r["redone"], true);
        let r = dispatch(&mut ctx, "history.redo", &json!({})).unwrap();
        assert_eq!(r["redone"], true);
        assert_eq!(*ctx.library, after_adds);
        // A fresh mutation clears the redo stack.
        let r = dispatch(&mut ctx, "history.undo", &json!({})).unwrap();
        assert_eq!(r["undone"], true);
        dispatch(
            &mut ctx,
            "item.add",
            &json!({"list_id": 1, "title": "Arrival"}),
        )
        .unwrap();
        let r = dispatch(&mut ctx, "history.redo", &json!({})).unwrap();
        assert_eq!(r["redone"], false);
        assert!(ctx
            .library
            .list(1)
            .unwrap()
            .items
            .iter()
            .any(|i| i.title == "Arrival"));
        assert!(!ctx
            .library
            .list(1)
            .unwrap()
            .items
            .iter()
            .any(|i| i.title == "Alien"));
    }

    #[test]
    fn history_empty_stack_is_a_clean_no_op() {
        let mut lib = Library::default();
        let mut history = History::new();
        let mut ctx = CommandContext::with_history(&mut lib, Path::new("."), &mut history);
        let r = dispatch(&mut ctx, "history.undo", &json!({})).unwrap();
        assert_eq!(r["undone"], false);
        let r = dispatch(&mut ctx, "history.redo", &json!({})).unwrap();
        assert_eq!(r["redone"], false);
        assert!(ctx.library.lists.is_empty());
        // And without a history attached at all: still a clean result.
        let mut lib2 = Library::default();
        let mut ctx2 = CommandContext::new(&mut lib2, Path::new("."));
        let r = dispatch(&mut ctx2, "history.undo", &json!({})).unwrap();
        assert_eq!(r["undone"], false);
    }

    #[test]
    fn history_failed_commands_record_nothing() {
        let mut lib = Library::default();
        let mut history = History::new();
        let mut ctx = CommandContext::with_history(&mut lib, Path::new("."), &mut history);
        dispatch(
            &mut ctx,
            "list.create",
            &json!({"name": "Films", "category": "movies"}),
        )
        .unwrap();
        assert_eq!(ctx.history.as_ref().unwrap().undo_len(), 1);
        let _ = dispatch(&mut ctx, "list.delete", &json!({"list_id": 99})).unwrap_err(); // not_found: no snapshot recorded
        assert_eq!(ctx.history.as_ref().unwrap().undo_len(), 1);
        let r = dispatch(&mut ctx, "history.undo", &json!({})).unwrap();
        assert_eq!(r["command"], "list.create");
        assert!(ctx.library.lists.is_empty());
    }

    #[test]
    fn bad_params_do_not_mutate() {
        let mut lib = Library::default();
        let mut ctx = CommandContext::new(&mut lib, Path::new("."));
        let err = dispatch(&mut ctx, "list.create", &json!({"name": "X"})).unwrap_err();
        assert_eq!(err.code, "bad_params"); // missing category
        let err = dispatch(
            &mut ctx,
            "list.create",
            &json!({"name": "X", "category": "anime"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "bad_params"); // not one of the 11 categories
        assert!(ctx.library.lists.is_empty());
    }
}
