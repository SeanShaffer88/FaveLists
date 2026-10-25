//! Headless JSON channel (v0.2 program #1): the binary launched with
//! `--json-channel` speaks JSON-lines over stdin/stdout instead of opening
//! the GUI.
//!
//! Protocol — one JSON object per line in, one per line out:
//!
//! * Request:  `{"command": "<name>", "params": {...}, "id": <any, optional>}`
//!   (`params` may be omitted for parameterless commands; `id` is echoed
//!   back verbatim so callers can correlate responses.)
//! * Success:  `{"ok": true, "id": <echo or null>, "result": {...}}`
//! * Failure:  `{"ok": false, "id": <echo or null>, "error": {"code": "...", "message": "..."}}`
//!
//! Requests dispatch through the same [`crate::commands`] registry the GUI
//! uses, against the store at the normal resolved data path (pointer file
//! honoured, exactly like the GUI). After every successful *mutating*
//! command the library is saved, so the channel and the GUI never diverge
//! on disk. A malformed line produces one `bad_json` error response and the
//! channel keeps going.
//!
//! The session owns a [`commands::History`] (Slice 2 #4), so
//! `history.undo`/`history.redo` work here exactly as in the GUI, and the
//! starting library is loaded through the journal-recovering path (a
//! corrupt main file with a valid journal no longer kills the session).

use crate::commands::{self, CommandContext, History};
use crate::store::Store;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

/// Run the channel until stdin reaches EOF. `base_dir` is the executable's
/// directory (the same base the GUI uses to resolve the data folder).
pub fn run(
    base_dir: &Path,
    mut input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let store = Store::open(base_dir)?;
    let mut library = store.load_library_recovering()?.library;
    let covers_dir = store.covers_dir();
    let mut history = History::new();
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = handle_line(&mut library, &covers_dir, &store, &mut history, trimmed);
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

fn error_response(id: Value, code: &str, message: &str) -> Value {
    json!({"ok": false, "id": id, "error": {"code": code, "message": message}})
}

fn handle_line(
    library: &mut crate::model::Library,
    covers_dir: &Path,
    store: &Store,
    history: &mut History,
    line: &str,
) -> Value {
    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return error_response(Value::Null, "bad_json", &format!("invalid JSON: {e}")),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(name) = request.get("command").and_then(Value::as_str) else {
        return error_response(id, "bad_request", "request needs a string 'command' field");
    };
    let name = name.to_string();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = {
        let mut ctx = CommandContext::with_history(library, covers_dir, history);
        commands::dispatch(&mut ctx, &name, &params)
    };
    match result {
        Ok(result) => {
            if commands::is_mutating(&name) {
                if let Err(e) = store.save_library(library) {
                    return error_response(id, "io", &format!("could not save library: {e}"));
                }
            }
            json!({"ok": true, "id": id, "result": result})
        }
        Err(e) => error_response(id, e.code, &e.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn temp_base(tag: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "favelists-channel-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run_session(base: &Path, input: &str) -> Vec<Value> {
        let mut out: Vec<u8> = Vec::new();
        run(base, Cursor::new(input.as_bytes().to_vec()), &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn session_dispatches_and_persists() {
        let base = temp_base("session");
        let input = concat!(
            "{\"command\":\"list.create\",\"params\":{\"name\":\"Films\",\"category\":\"movies\",\"ranked\":true},\"id\":1}\n",
            "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Alien\",\"fields\":{\"year\":\"1979\"},\"added_at\":1700000000},\"id\":2}\n",
            "{\"command\":\"library.stats\",\"params\":{},\"id\":3}\n",
        );
        let responses = run_session(&base, input);
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["ok"], true);
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(responses[0]["result"]["list_id"], 1);
        assert_eq!(responses[1]["result"]["outcome"], "added");
        assert_eq!(responses[2]["result"]["items"], 1);
        // The mutating commands were persisted to the normal data path.
        let store = Store::open(&base).unwrap();
        let lib = store.load_library().unwrap();
        assert_eq!(lib.lists.len(), 1);
        assert_eq!(lib.lists[0].items[0].title, "Alien");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn bad_lines_error_and_the_channel_continues() {
        let base = temp_base("badlines");
        let input = concat!(
            "this is not json\n",
            "{\"params\":{}}\n",
            "{\"command\":\"nope.nope\",\"params\":{}}\n",
            "{\"command\":\"list.get\",\"params\":{\"list_id\":99}}\n",
            "{\"command\":\"library.stats\"}\n",
        );
        let responses = run_session(&base, input);
        assert_eq!(responses.len(), 5);
        assert_eq!(responses[0]["error"]["code"], "bad_json");
        assert_eq!(responses[1]["error"]["code"], "bad_request");
        assert_eq!(responses[2]["error"]["code"], "unknown_command");
        assert_eq!(responses[3]["error"]["code"], "not_found");
        assert_eq!(responses[4]["ok"], true);
        assert_eq!(responses[4]["result"]["lists"], 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}
