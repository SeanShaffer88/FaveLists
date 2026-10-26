//! v0.3 Slice 3 — CSV/JSON export (#6), paste-a-list import (#7) and
//! fuzzy duplicates (#8), all through the command registry.

use favelists::commands::{self, CommandContext, History};
use favelists::model::Library;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("favelists-data-io-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn dispatch(
    lib: &mut Library,
    covers: &Path,
    name: &str,
    params: Value,
) -> Result<Value, commands::CommandError> {
    let mut ctx = CommandContext::new(lib, covers);
    commands::dispatch(&mut ctx, name, &params)
}

fn dispatch_h(
    lib: &mut Library,
    covers: &Path,
    history: &mut History,
    name: &str,
    params: Value,
) -> Result<Value, commands::CommandError> {
    let mut ctx = CommandContext::with_history(lib, covers, history);
    commands::dispatch(&mut ctx, name, &params)
}

fn make_list(lib: &mut Library, covers: &Path, name: &str, category: &str) -> u64 {
    dispatch(
        lib,
        covers,
        "list.create",
        json!({"name": name, "category": category}),
    )
    .unwrap()["list_id"]
        .as_u64()
        .unwrap()
}

fn add(lib: &mut Library, covers: &Path, list_id: u64, title: &str, year: &str) {
    dispatch(
        lib,
        covers,
        "item.add",
        json!({"list_id": list_id, "title": title, "fields": {"year": year}}),
    )
    .unwrap();
}

// ---------------------------------------------------------------- #6 export

#[test]
fn export_csv_exact_bytes_with_quoting() {
    let dir = temp_dir("csv-bytes");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Top Movies", "movies");
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({
            "list_id": list, "title": "The Shawshank Redemption",
            "fields": {"year": "1994", "director": "Frank Darabont"},
            "rating": 9,
            "note": "A \"classic\",\nstill great",
            "tags": ["drama", "top, all-time"],
            "status": "tried",
            "provider_id": "278", "source": "tmdb",
            "added_at": 1700000000u64,
        }),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({
            "list_id": list, "title": "Alien",
            "fields": {"year": "1979", "director": "Ridley Scott", "genre": "Horror"},
            "rating": 10,
            "added_at": 1700000100u64,
        }),
    )
    .unwrap();

    let path = dir.join("movies.csv");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.csv",
        json!({"list_id": list, "path": path.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(r["rows"], 2);
    // `written` is the house bool (as in the text exports), not a byte
    // count; the byte-exact check below covers the content.
    assert_eq!(r["written"], true);

    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes, expected_movies_csv().as_bytes());
}

fn expected_movies_csv() -> String {
    let mut s = String::new();
    // Columns follow the Movies field set: year, genre, director,
    // runtime, cast — then the tail columns.
    s.push_str("title,year,genre,director,runtime,cast,rating,note,tags,status,provider_id,source,added_at\r\n");
    s.push_str("The Shawshank Redemption,1994,,Frank Darabont,,,9,");
    s.push_str("\"A \"\"classic\"\",\nstill great\",");
    s.push_str("\"drama, top, all-time\",tried,278,tmdb,1700000000\r\n");
    s.push_str("Alien,1979,Horror,Ridley Scott,,,10,,,,,manual,1700000100\r\n");
    s
}

#[test]
fn export_library_json_round_trips_to_an_equal_library() {
    let dir = temp_dir("lib-json");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let movies = make_list(&mut lib, &covers, "Movies", "movies");
    add(&mut lib, &covers, movies, "Alien", "1979");
    let sodas = make_list(&mut lib, &covers, "Sodas", "sodas");
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": sodas, "title": "Cola",
               "fields": {"brand": "FizzCo", "flavor": "Cola"}}),
    )
    .unwrap();

    let path = dir.join("library.json");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.library",
        json!({"format": "json", "path": path.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(r["lists"], 2);
    assert_eq!(r["items"], 2);

    let text = fs::read_to_string(&path).unwrap();
    let back: Library = serde_json::from_str(&text).unwrap();
    assert_eq!(back, lib);
}

#[test]
fn export_library_csv_headers_and_rows() {
    let dir = temp_dir("lib-csv");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let movies = make_list(&mut lib, &covers, "Movies", "movies");
    add(&mut lib, &covers, movies, "Alien", "1979");
    let sodas = make_list(&mut lib, &covers, "Sodas", "sodas");
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": sodas, "title": "Cola",
               "fields": {"brand": "FizzCo", "flavor": "Cola"}}),
    )
    .unwrap();

    let path = dir.join("library.csv");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.library",
        json!({"format": "csv", "path": path.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(r["lists"], 2);
    assert_eq!(r["items"], 2);

    let text = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.trim_end_matches("\r\n").split("\r\n").collect();
    assert_eq!(lines.len(), 3, "header + one row per item: {text}");
    // Columns: the union of the present categories' field sets, in
    // Category::ALL order (Movies: year, genre, director, runtime,
    // cast; Sodas: brand, flavor). No item here carries an extra field
    // (e.g. caffeine), so no extra column appears.
    assert_eq!(
        lines[0],
        "list,category,title,year,genre,director,runtime,cast,brand,flavor,\
         rating,note,tags,status,provider_id,source,added_at"
    );
    // The soda row puts its brand in the brand column (after the five
    // movie field columns).
    let soda_row = lines
        .iter()
        .find(|l| l.starts_with("Sodas,"))
        .expect("soda row present");
    let cells: Vec<&str> = soda_row.split(',').collect();
    assert_eq!(cells[0], "Sodas");
    assert_eq!(cells[1], "sodas");
    assert_eq!(cells[2], "Cola");
    assert_eq!(cells[8], "FizzCo");
    assert_eq!(cells[9], "Cola");
}

#[test]
fn export_errors() {
    let dir = temp_dir("export-errors");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    let path = dir.join("out.txt");

    let e = dispatch(
        &mut lib,
        &covers,
        "export.library",
        json!({"format": "xml", "path": path.to_string_lossy()}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");

    let e = dispatch(
        &mut lib,
        &covers,
        "export.csv",
        json!({"list_id": 999, "path": path.to_string_lossy()}),
    )
    .unwrap_err();
    assert_eq!(e.code, "not_found");

    let e = dispatch(&mut lib, &covers, "export.csv", json!({"list_id": list})).unwrap_err();
    assert_eq!(e.code, "bad_params");

    let e = dispatch(
        &mut lib,
        &covers,
        "export.library",
        json!({"format": "csv"}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");

    // Export commands never mutate.
    assert!(!commands::is_mutating("export.csv"));
    assert!(!commands::is_mutating("export.library"));
    assert!(commands::is_mutating("import.paste"));
}

// ---------------------------------------------------------------- #7 paste

#[test]
fn paste_adds_skips_and_counts() {
    let dir = temp_dir("paste-basic");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    add(&mut lib, &covers, list, "Alien", "1979");

    let r = dispatch(
        &mut lib,
        &covers,
        "import.paste",
        json!({
            "target": {"mode": "existing", "list_id": list},
            "rows": [
                {"title": "Dune", "fields": {"year": "2021"},
                 "provider_id": "438631", "source": "tmdb"},
                // Dup within the paste: same provider identity, so the
                // model's work_key rule treats it as the same work —
                // exactly as item.add / the CSV import treat it.
                {"title": "Dune", "fields": {"year": "2021"},
                 "provider_id": "438631", "source": "tmdb"},
                {"title": "Alien", "fields": {"year": "1979"}}, // already present
                {"title": "   "},                               // blank
                {"title": "Arrival", "fields": {"year": "2016"}},
            ],
        }),
    )
    .unwrap();
    assert_eq!(r["added"], 2);
    assert_eq!(r["skipped_duplicates"], 2);
    assert_eq!(r["skipped_empty"], 1);
    assert_eq!(r["invalid"], 1);
    assert_eq!(r["lists"][0]["added"], 2);

    let list_ref = lib.list(list).unwrap();
    assert_eq!(list_ref.items.len(), 3);
    let dune = list_ref.items.iter().find(|i| i.title == "Dune").unwrap();
    assert_eq!(dune.provider_id.as_deref(), Some("438631"));
    assert_eq!(dune.source, "tmdb");
    assert_eq!(dune.field("year"), Some("2021"));
}

#[test]
fn paste_into_a_new_list_and_one_undo_step() {
    let dir = temp_dir("paste-new");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let mut history = History::default();

    let r = dispatch_h(
        &mut lib,
        &covers,
        &mut history,
        "import.paste",
        json!({
            "target": {"mode": "new", "name": "Pasted Books", "category": "books"},
            "rows": [{"title": "Dune"}, {"title": "Foundation"}],
        }),
    )
    .unwrap();
    assert_eq!(r["added"], 2);
    assert_eq!(lib.lists.len(), 1);
    assert_eq!(lib.lists[0].items.len(), 2);

    // ONE dispatch = ONE undo step: a single undo removes the whole paste
    // (including the list it created); a single redo restores it.
    dispatch_h(&mut lib, &covers, &mut history, "history.undo", json!({})).unwrap();
    assert!(lib.lists.is_empty());
    dispatch_h(&mut lib, &covers, &mut history, "history.redo", json!({})).unwrap();
    assert_eq!(lib.lists.len(), 1);
    assert_eq!(lib.lists[0].items.len(), 2);
}

#[test]
fn paste_errors() {
    let dir = temp_dir("paste-errors");
    let covers = dir.join("covers");
    let mut lib = Library::default();

    // A new-list target needs a category (rows carry none).
    let e = dispatch(
        &mut lib,
        &covers,
        "import.paste",
        json!({"target": {"mode": "new", "name": "X"}, "rows": [{"title": "Dune"}]}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");

    // Unknown existing list.
    let e = dispatch(
        &mut lib,
        &covers,
        "import.paste",
        json!({"target": {"mode": "existing", "list_id": 42}, "rows": [{"title": "Dune"}]}),
    )
    .unwrap_err();
    assert_eq!(e.code, "not_found");

    // Over the 500-row cap.
    let rows: Vec<Value> = (0..501)
        .map(|i| json!({"title": format!("Title {i}")}))
        .collect();
    let e = dispatch(
        &mut lib,
        &covers,
        "import.paste",
        json!({"target": {"mode": "new", "name": "Big", "category": "books"}, "rows": rows}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");
    assert!(lib.lists.is_empty(), "a rejected paste changes nothing");
}

// ---------------------------------------------------------------- #8 fuzzy

fn find_groups(lib: &mut Library, covers: &Path) -> Value {
    dispatch(lib, covers, "library.find_duplicates", json!({})).unwrap()["groups"].clone()
}

#[test]
fn fuzzy_part_ii_groups_with_part_2_as_a_title_match() {
    let dir = temp_dir("fz-part2");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    add(&mut lib, &covers, list, "Rocky Part II", "1979");
    add(&mut lib, &covers, list, "Rocky Part 2", "1979");
    let groups = find_groups(&mut lib, &covers);
    assert_eq!(groups.as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["match_kind"], "title");
    assert_eq!(groups[0]["confidence"], "exact");
    assert_eq!(groups[0]["copies"].as_array().unwrap().len(), 2);
}

#[test]
fn fuzzy_directors_cut_variant_groups_as_a_title_match() {
    let dir = temp_dir("fz-cut");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    add(&mut lib, &covers, list, "Blade Runner", "1982");
    add(
        &mut lib,
        &covers,
        list,
        "Blade Runner: Director's Cut",
        "1982",
    );
    let groups = find_groups(&mut lib, &covers);
    assert_eq!(groups.as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["match_kind"], "title");
    assert_eq!(groups[0]["confidence"], "exact");
}

#[test]
fn fuzzy_typo_pair_groups_as_similar_possible_and_merges() {
    let dir = temp_dir("fz-typo");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    add(&mut lib, &covers, list, "The Shawshank Redemption", "1994");
    add(&mut lib, &covers, list, "The Shawshank Redemtion", "1994");
    let groups = find_groups(&mut lib, &covers);
    assert_eq!(groups.as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["match_kind"], "similar");
    assert_eq!(groups[0]["confidence"], "possible");
    // The group key is the normalised title with the leading article
    // stripped, as in the exact/possible finder since v0.2.
    assert_eq!(groups[0]["key"], "shawshank redemption");

    // Merge works on fuzzy groups unchanged (same keep/drop ref
    // shape as the exact-match merge since v0.2).
    let group = groups[0].clone();
    let keep = &group["copies"][0];
    let drop = &group["copies"][1];
    let r = dispatch(
        &mut lib,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": keep["list_id"], "item_id": keep["item_id"]},
               "drop": [{"list_id": drop["list_id"], "item_id": drop["item_id"]}]}),
    )
    .unwrap();
    assert_eq!(r["removed"].as_array().unwrap().len(), 1);
    assert_eq!(lib.list(list).unwrap().items.len(), 1);
}

#[test]
fn fuzzy_sequels_never_group() {
    let dir = temp_dir("fz-sequels");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    // Same years on purpose: only the no-token-subset rule keeps these
    // apart (and "Dune" is too short for the distance bound anyway).
    add(&mut lib, &covers, list, "The Matrix", "1999");
    add(&mut lib, &covers, list, "The Matrix Reloaded", "1999");
    add(&mut lib, &covers, list, "Dune", "2021");
    add(&mut lib, &covers, list, "Dune 2", "2021");
    let groups = find_groups(&mut lib, &covers);
    assert!(groups.as_array().unwrap().is_empty(), "groups: {groups}");
}

#[test]
fn fuzzy_year_rules() {
    let dir = temp_dir("fz-years");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Movies", "movies");
    // Typo pair, years seven apart: NOT grouped.
    add(&mut lib, &covers, list, "The Shawshank Redemption", "1994");
    add(&mut lib, &covers, list, "The Shawshank Redemtion", "2001");
    // Typo pair, years one apart: grouped as similar/possible.
    add(&mut lib, &covers, list, "The Godfather", "1972");
    add(&mut lib, &covers, list, "The Godfater", "1973");
    let groups = find_groups(&mut lib, &covers);
    assert_eq!(groups.as_array().unwrap().len(), 1, "groups: {groups}");
    assert_eq!(groups[0]["match_kind"], "similar");
    assert_eq!(groups[0]["confidence"], "possible");
    let mut years: Vec<&str> = groups[0]["copies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["year"].as_str().unwrap())
        .collect();
    years.sort_unstable();
    assert_eq!(years, ["1972", "1973"]);
}
