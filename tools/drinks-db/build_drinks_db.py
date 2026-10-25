#!/usr/bin/env python3
"""Build the FaveLists drinks database pack from an Open Food Facts export.

Pipeline (spec section 6):
  stream the OFF CSV export (tab-separated, may be .gz) -> keep only
  US + English products tagged en:sodas or en:energy-drinks -> map to
  {name, brand, flavor, kind, image_url} -> dedupe by
  kind+brand+product+flavor -> write a compact JSON pack the app bundles.

The filter/dedupe rules mirror src/drinks.rs in the app (the Rust side is
unit-tested; this script has a --selftest with the same expectations).

Usage:
  python3 build_drinks_db.py <export.csv[.gz]> <out.json> [--version N]
  python3 build_drinks_db.py --selftest

Data license: Open Food Facts database is ODbL; product images are
CC BY-SA. Keep the attribution in the output pack's "source" field.
"""
import csv
import gzip
import json
import re
import sys

csv.field_size_limit(64 * 1024 * 1024)

GENERICS = ["energy drink", "energy", "soda", "drink", "beverage"]


def normalise(s: str) -> str:
    s = re.sub(r"[^0-9a-z]+", " ", s.lower())
    return re.sub(r"\s+", " ", s).strip()


def parse_flavor(name: str, brand: str) -> str:
    """Mirror of favelists::drinks::parse_flavor."""
    rest = name.strip()
    if brand:
        idx = rest.lower().find(brand.lower())
        if idx >= 0:
            rest = (rest[:idx] + rest[idx + len(brand):]).strip()
    if normalise(rest) == normalise(name):
        return ""
    changed = True
    while changed:
        changed = False
        lower = rest.lower()
        for g in GENERICS:
            if lower == g:
                rest = ""
                changed = True
                break
            if lower.endswith(g):
                stripped = lower[: -len(g)]
                if stripped.endswith((" ", "-")):
                    rest = rest[: len(stripped.rstrip())].strip()
                    changed = True
                    break
        trimmed = rest.strip("-· ")
        if trimmed != rest:
            rest = trimmed
            changed = True
    return rest


def filter_row(row: dict):
    """Return a pack entry dict, or None when the row is out of scope."""
    name = (row.get("product_name") or "").strip()
    if not name:
        return None
    # The en.* export flavour has no lang column (names are English by
    # construction); when a lang column exists it must say "en".
    lang = (row.get("lang") or "").strip().lower()
    if lang not in ("", "en"):
        return None
    countries = (row.get("countries_tags") or "").lower()
    if "en:united-states" not in countries:
        return None
    cats = (row.get("categories_tags") or "").lower()
    if "en:energy-drinks" in cats:
        kind = "EnergyDrink"
    elif "en:sodas" in cats:
        kind = "Soda"
    else:
        return None
    brand = (row.get("brands") or "").split(",")[0].strip()
    image = (row.get("image_front_url") or row.get("image_url") or "").strip()
    return {
        "name": name,
        "brand": brand,
        "flavor": parse_flavor(name, brand),
        "kind": kind,
        "image_url": image or None,
    }


def brands_compatible(a: str, b: str) -> bool:
    a, b = normalise(a), normalise(b)
    return not a or not b or a == b or a in b or b in a


def dedupe(entries):
    seen = {}
    out = []
    for e in entries:
        key = "|".join([
            e["kind"],
            normalise(e["brand"]),
            normalise(e["name"]),
            normalise(e["flavor"]),
        ])
        if key in seen:
            i = seen[key]
            if not out[i]["image_url"] and e["image_url"]:
                out[i]["image_url"] = e["image_url"]
        else:
            seen[key] = len(out)
            out.append(e)
    # Collapse pass: same product (kind+name+flavor) under compatible brand
    # strings ("Coca-Cola" vs "The Coca-Cola Company") shows once. Bucketed
    # by the product key so this stays fast on the full export.
    buckets = {}
    result = []
    for e in out:
        bkey = (e["kind"], normalise(e["name"]), normalise(e["flavor"]))
        match = None
        for r in buckets.get(bkey, []):
            if brands_compatible(r["brand"], e["brand"]):
                match = r
                break
        if match is None:
            buckets.setdefault(bkey, []).append(e)
            result.append(e)
        else:
            if not match["image_url"] and e["image_url"]:
                match["image_url"] = e["image_url"]
            if e["brand"] and len(e["brand"]) < len(match["brand"]):
                match["brand"] = e["brand"]
    return result


def build(input_path: str, version: int) -> dict:
    opener = gzip.open if input_path.endswith(".gz") else open
    entries = []
    with opener(input_path, "rt", encoding="utf-8", errors="replace", newline="") as fh:
        reader = csv.DictReader(fh, delimiter="\t")
        for row in reader:
            entry = filter_row(row)
            if entry is not None:
                entries.append(entry)
    entries = dedupe(entries)
    entries.sort(key=lambda e: e["name"])
    return {
        "version": version,
        "source": "Open Food Facts export (ODbL); images CC BY-SA",
        "entries": entries,
    }


def selftest() -> int:
    base = {
        "product_name": "Coca-Cola Classic",
        "brands": "Coca-Cola",
        "categories_tags": "en:beverages,en:sodas",
        "countries_tags": "en:united-states",
        "lang": "en",
        "image_front_url": "https://img.example/x.jpg",
    }
    e = filter_row(base)
    assert e and e["kind"] == "Soda" and e["brand"] == "Coca-Cola" and e["flavor"] == "Classic", e
    assert filter_row({**base, "countries_tags": "en:france"}) is None
    assert filter_row({**base, "lang": "fr"}) is None
    assert filter_row({**base, "categories_tags": "en:chips"}) is None
    assert parse_flavor("Fanta Orange Soda", "Fanta") == "Orange"
    assert parse_flavor("Pepsi", "Pepsi") == ""
    merged = dedupe([e, {**e, "image_url": None}, {**e, "name": "Other"}])
    assert len(merged) == 2 and merged[0]["image_url"], merged
    print("selftest OK")
    return 0


def main(argv) -> int:
    if "--selftest" in argv:
        return selftest()
    version = 1
    argv = list(argv)
    if "--version" in argv:
        i = argv.index("--version")
        version = int(argv[i + 1])
        del argv[i : i + 2]
    args = argv[1:]
    if len(args) != 2:
        print(__doc__)
        return 2
    pack = build(args[0], version)
    with open(args[1], "w", encoding="utf-8") as fh:
        json.dump(pack, fh, ensure_ascii=False, separators=(",", ":"))
    sodas = sum(1 for e in pack["entries"] if e["kind"] == "Soda")
    energy = sum(1 for e in pack["entries"] if e["kind"] == "EnergyDrink")
    print(f"wrote {args[1]}: {len(pack['entries'])} entries ({sodas} sodas, {energy} energy drinks), version {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
