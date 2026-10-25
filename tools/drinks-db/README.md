# Drinks database pipeline (Energy Drinks + Sodas)

FaveLists' two drinks categories search a **bundled offline database**
instead of a live API (spec §6). This folder builds that database.

## Source

[Open Food Facts](https://world.openfoodfacts.org) nightly export
(`en.openfoodfacts.org.products.csv.gz`, tab-separated CSV, ~1.5 GB gz).
The database is licensed **ODbL**; product images are **CC BY-SA** — the
attribution is carried in the pack's `source` field and in the app README.
USDA FoodData Central branded foods may supplement names/brands in future
passes. Brand and retailer websites are cross-check sources only — they
are never bulk-scraped.

## What the filter keeps

- `lang` is English (the `en.*` export flavour carries no `lang` column;
  its product names are English by construction — an empty lang passes)
- `countries_tags` contains `en:united-states`
- `categories_tags` contains `en:energy-drinks` (→ EnergyDrink) or
  `en:sodas` (→ Soda)
- a non-empty product name

Each kept row becomes `{name, brand, flavor, kind, image_url}` where brand
is the first entry of OFF's comma brand list and flavor is a best-effort
parse (name minus brand minus generic words like "Energy Drink"/"Soda").
Rows are deduped by kind + brand + product + flavor, preferring the entry
that has an image, and sorted by name.

The image itself is NOT bundled (that would explode the pack size): the
pack stores `image_url`, and the app downloads it once when a drink is
added to a list, then serves it from the local cover cache.

## Running it

```sh
python3 build_drinks_db.py export.csv.gz ../../data/drinks.json --version N
python3 build_drinks_db.py --selftest
```

The Python tool streams the export, so the multi-GB decompressed CSV never
lands on disk. A Rust twin (`cargo run --bin drinks-db-build -- export.csv
out.json N`, stdin with `-`) shares the exact filter/dedupe implementation
in `src/drinks.rs`, which is where the unit tests live.

## Updates at runtime

The app treats the bundled file as version N. A downloaded pack (same JSON
shape) with a strictly higher version, placed/retrieved via the Settings
"Update drinks database" flow, replaces it (stored as `drinks.json` in the
data folder). Silent auto-update checks the configured URL at most once a
day; no update server exists yet, so the URL ships empty.
