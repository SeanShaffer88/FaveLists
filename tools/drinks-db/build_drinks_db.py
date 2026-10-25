#!/usr/bin/env python3
"""Build the FaveLists drinks database pack from an Open Food Facts export.

Pipeline (spec section 6):
  stream the OFF CSV export (tab-separated, may be .gz) -> keep only
  US + English products tagged en:sodas or en:energy-drinks -> map to
  {name, brand, flavor, kind, image_url} -> dedupe by
  kind+brand+product+flavor -> write a compact JSON pack the app bundles.

The filter/dedupe rules mirror src/drinks.rs in the app (the Rust side is
unit-tested; this script has a --selftest with the same expectations).

v0.2 #7 quality pass: flavour extraction derives flavours from product
names far more often (comma-tail segments + brand-anchored head mining),
soda-tagged rows with positive energy evidence (an "energy" name token
or a documented energy brand, see ENERGY_BRANDS) are classified as
energy drinks, and rows carrying the OFF `caffeine_100g` nutriment gain
an optional `caffeine_mg_per_100ml` pack field.

Usage:
  python3 build_drinks_db.py <export.csv[.gz]> <out.json> [--version N]
  python3 build_drinks_db.py --selftest

Data license: Open Food Facts database is ODbL; product images are
CC BY-SA. Keep the attribution in the output pack's "source" field.
"""
import csv
import gzip
import json
import math
import re
import sys

csv.field_size_limit(64 * 1024 * 1024)


def normalise(s: str) -> str:
    s = re.sub(r"[^0-9a-z]+", " ", s.lower())
    return re.sub(r"\s+", " ", s).strip()


# --- Flavour extraction (v0.2 #7) -------------------------------------------
# Mirrors favelists::drinks::parse_flavor exactly; the --selftest below
# and the Rust unit tests run the SAME vector table.

# Descriptor phrases stripped during extraction, longest first wherever
# one phrase contains another ("energy drink" before "energy",
# "flavoured" before "flavour"). Keep in sync with the Rust twin.
FLAVOR_DESCRIPTORS = [
    "regular strength", "extra strength", "caffeine free", "energy drink",
    "zero calorie", "calorie free", "energy shot", "low calorie",
    "zero sugar", "sugar free", "soft drink", "carbonated", "naturally", "sparkling",
    "beverage", "flavored", "flavoured", "premium", "energy", "flavor",
    "flavour", "drink", "soda", "diet", "mix",
]

SIZE_UNITS = {
    "fl", "oz", "ml", "cl", "l", "g", "kg", "pack", "pk", "ct", "count",
    "liter", "liters", "litre", "litres",
}
FUSED_SIZE = re.compile(r"^(\d+(?:\.\d+)?)(floz|oz|ml|cl|l|g|kg)$")
PACK_SIZE = re.compile(r"^(\d+(?:\.\d+)?)-(pack|pk|ct)$")
BARE_NUMBER = re.compile(r"^\d+(\.\d+)?$")
TOKEN = re.compile(r"[^\W_]+")


def _remove_phrase_ci(text: str, phrase: str) -> str:
    out = []
    i, n, m = 0, len(text), len(phrase)
    if m == 0 or n < m:
        return text
    while i < n:
        if (
            text[i : i + m].lower() == phrase
            and (i == 0 or not text[i - 1].isalnum())
            and (i + m >= n or not text[i + m].isalnum())
        ):
            i += m
            continue
        out.append(text[i])
        i += 1
    return "".join(out)


def _strip_size_tokens(segment: str) -> str:
    toks = segment.split()
    out = []
    i = 0
    while i < len(toks):
        low = toks[i].lower().strip(",;-·|/")
        if FUSED_SIZE.match(low) or PACK_SIZE.match(low):
            i += 1
            continue
        if BARE_NUMBER.match(low):
            nxt = toks[i + 1].lower().strip(",;-·|/") if i + 1 < len(toks) else ""
            nxt2 = toks[i + 2].lower().strip(",;-·|/") if i + 2 < len(toks) else ""
            if nxt == "fl" and nxt2 == "oz":
                i += 3
                continue
            if nxt in SIZE_UNITS:
                i += 2
                continue
        out.append(toks[i])
        i += 1
    return " ".join(out)


def _clean_flavor_text(segment: str) -> str:
    s = segment.replace("-", " ").replace("·", " ").replace("|", " ")
    s = _strip_size_tokens(s)
    for _ in range(2):
        for phrase in FLAVOR_DESCRIPTORS:
            s = _remove_phrase_ci(s, phrase)
    s = _strip_size_tokens(s)
    return " ".join(s.split()).strip(" ,;")


def _strip_leading_brand_tokens(seg: str, brand: str):
    brand_tokens = {t.lower() for t in TOKEN.findall(brand)}
    if not brand_tokens:
        return seg, False
    end = 0
    count = 0
    for m in TOKEN.finditer(seg):
        if m.group(0).lower() in brand_tokens:
            end = m.end()
            count += 1
        else:
            break
    if count == 0:
        return seg, False
    return seg[end:], True


def _title_case(s: str) -> str:
    def part(p: str) -> str:
        return p[:1].upper() + p[1:].lower() if p else p

    return " ".join("-".join(part(p) for p in w.split("-")) for w in s.split(" "))


def parse_flavor(name: str, brand: str) -> str:
    """Mirror of favelists::drinks::parse_flavor (v0.2 #7 algorithm).

    Comma segments: the flavour is the last tail segment (after the
    first) that survives cleaning; otherwise the first segment is mined
    when a brand anchor exists (brand substring, or leading brand
    tokens). Cleaning strips size tokens and descriptor phrases.
    Nothing is invented: no rule firing means an empty flavour.
    """
    rest = name.strip()
    brand_found = False
    if brand:
        idx = rest.lower().find(brand.lower())
        if idx >= 0:
            rest = rest[:idx] + rest[idx + len(brand):]
            brand_found = True
    segments = [s.strip() for s in rest.split(",")]
    segments = [s for s in segments if s]
    if not segments:
        return ""
    tail = [c for c in (_clean_flavor_text(s) for s in segments[1:]) if c]
    if tail:
        result = tail[-1]
    else:
        seg, stripped = _strip_leading_brand_tokens(segments[0], brand)
        if not brand_found and not stripped:
            return ""
        result = _clean_flavor_text(seg)
    if not result or normalise(result) == normalise(name):
        return ""
    return _title_case(result)


# --- Classification evidence (v0.2 #7) ---------------------------------------
# Documented energy-drink brands (normalised), used as positive evidence.
# Derived 2026-10-25 from the bundled pack itself (brands whose entries
# were >=90% energy drinks across >=3 entries) plus the canonical energy
# brands whose OFF rows sit under longer company strings. Glaceau and
# Powerade were excluded on judgement (primarily vitamin/sports-drink
# brands). Keep in sync with the Rust twin.
ENERGY_BRANDS = {
    "1st phorm", "3d energy", "5 hour energy", "alani", "alani nu",
    "applied nutrition", "arizona super lxr", "bang", "bang energy",
    "bucked up energy", "bum", "c4", "celcius", "celsius", "clean cause",
    "fitaid", "flrt", "full throttle", "gfuel", "ghost", "gorilla mind",
    "gorgie", "jocko", "lotus", "monster energy", "nos", "oca",
    "optimum nutrition", "proper wild", "prox", "red bull", "reign",
    "rockstar", "vivee", "zoa",
}


def has_energy_evidence(name: str, brand: str) -> bool:
    """Mirror of favelists::drinks::has_energy_evidence."""
    if "energy" in TOKEN.findall(name.lower()):
        return True
    return normalise(brand) in ENERGY_BRANDS


def caffeine_mg_per_100ml(caffeine_100g):
    """Mirror of favelists::drinks::caffeine_mg_per_100ml.

    OFF reports caffeine in grams per 100 g; drinks are ~1 g/ml, so
    x1000 gives mg per 100 ml. Implausible values are dropped; the
    result is rounded to one decimal (half away from zero, like Rust).
    """
    if caffeine_100g is None:
        return None
    mg = caffeine_100g * 1000.0
    if not (1.0 <= mg <= 400.0):
        return None
    return math.floor(mg * 10.0 + 0.5) / 10.0


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
    brand = (row.get("brands") or "").split(",")[0].strip()
    cats = (row.get("categories_tags") or "").lower()
    if "en:energy-drinks" in cats:
        kind = "EnergyDrink"
    elif "en:sodas" in cats:
        # v0.2 #7: OFF tags stay primary, but a soda-tagged row with
        # positive energy evidence in its name/brand is an energy drink.
        kind = "EnergyDrink" if has_energy_evidence(name, brand) else "Soda"
    else:
        return None
    image = (row.get("image_front_url") or row.get("image_url") or "").strip()
    entry = {
        "name": name,
        "brand": brand,
        "flavor": parse_flavor(name, brand),
        "kind": kind,
        "image_url": image or None,
    }
    caffeine_raw = (row.get("caffeine_100g") or "").strip()
    try:
        caffeine_g = float(caffeine_raw) if caffeine_raw else None
    except ValueError:
        caffeine_g = None
    caffeine = caffeine_mg_per_100ml(caffeine_g)
    if caffeine is not None:
        entry["caffeine_mg_per_100ml"] = caffeine
    return entry


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
            if "caffeine_mg_per_100ml" not in out[i] and "caffeine_mg_per_100ml" in e:
                out[i]["caffeine_mg_per_100ml"] = e["caffeine_mg_per_100ml"]
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
            if "caffeine_mg_per_100ml" not in match and "caffeine_mg_per_100ml" in e:
                match["caffeine_mg_per_100ml"] = e["caffeine_mg_per_100ml"]
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
    assert "caffeine_mg_per_100ml" not in e, e
    assert filter_row({**base, "countries_tags": "en:france"}) is None
    assert filter_row({**base, "lang": "fr"}) is None
    assert filter_row({**base, "categories_tags": "en:chips"}) is None
    # Flavour vectors — the SAME table runs in the Rust unit tests
    # (src/drinks.rs flavor_extraction_vectors_match_python_twin).
    vectors = [
        ("Coca-Cola Classic", "Coca-Cola", "Classic"),
        ("Pepsi", "Pepsi", ""),
        ("Pepsi", "Pepsi-Cola", ""),
        ("Ghost Energy Drink", "Ghost", ""),
        ("Fanta Orange Soda", "Fanta", "Orange"),
        ("!Ajua!, Caffeine Free Soda, Mandarin Orange", "!Ajua!", "Mandarin Orange"),
        ("1 Liter Pepsi", "Pepsi", ""),
        ("3D Citrus Mist Energy Drink", "3D Energy", "Citrus Mist"),
        ("5-hour Energy Regular Strength Berry 1.93oz", "5-hour Energy", "Berry"),
        ("5-hour ENERGY Extra Strength Hawaiian Breeze", "5-hour ENERGY", "Hawaiian Breeze"),
        ("Bang Peach Mango", "bang energy", "Peach Mango"),
        ("Bang sour heads", "Bang", "Sour Heads"),
        ("Amazon Energy", "Sambazon", ""),
        ("51 Fifty, Energy Drink, Fruit Punch", "51 Fifty Energy", "Fruit Punch"),
        ("Amp energy, mixed berry energy drink", "", "Mixed Berry"),
        ("Coca-Cola Cherry", "The Coca-Cola Company", "Cherry"),
        ("ACTIVE Essential Amino Acid Blend", "OPTAVIA", ""),
        ("Monster Energy Ultra Sunrise", "Monster Energy", "Ultra Sunrise"),
        ("Monster Energy Ultra Sunrise 16 fl oz", "Monster Energy", "Ultra Sunrise"),
        ("51 Fifty, Sugar Free Energy Drink", "51 Fifty Energy", ""),
        ("Dr Pepper Zero Sugar", "Dr Pepper", ""),
        ("Fanta Orange 12 fl oz", "", ""),
        ("Fanta, Orange", "", "Orange"),
        ("Ahold, diet root beer", "Ahold", "Root Beer"),
        ("A' siciliana, sicily blood orange soda", "A' Siciliana", "Sicily Blood Orange"),
        ("Diet Coke", "Coca-Cola", ""),
        ("Coca-Cola, Cherry Zero Sugar, 12 pack", "Coca-Cola", "Cherry"),
        ("Gatorade Frost Glacier Freeze", "Gatorade", "Frost Glacier Freeze"),
        ("7up, caffeine free naturally flavored soda", "", ""),
        ("2 Gingers, Premium Ginger Ale", "Polar Corporation", "Ginger Ale"),
    ]
    for name, brand, want in vectors:
        got = parse_flavor(name, brand)
        assert got == want, f"flavour of {name!r} (brand {brand!r}): got {got!r}, want {want!r}"
    # Classification: soda tag + positive evidence -> energy drink.
    by_name = filter_row({**base, "product_name": "Amazon Energy", "brands": "Sambazon"})
    assert by_name and by_name["kind"] == "EnergyDrink", by_name
    by_brand = filter_row({**base, "product_name": "Bang Peach Mango", "brands": "bang energy"})
    assert by_brand and by_brand["kind"] == "EnergyDrink", by_brand
    assert filter_row(base)["kind"] == "Soda"
    tagged = filter_row({**base, "product_name": "Mystery Drink", "brands": "Generic",
                         "categories_tags": "en:beverages,en:energy-drinks"})
    assert tagged and tagged["kind"] == "EnergyDrink", tagged
    # Caffeine: g/100g -> mg/100ml, plausibility window, absent otherwise.
    assert caffeine_mg_per_100ml(0.032) == 32.0
    assert caffeine_mg_per_100ml(0.0315) == 31.5
    assert caffeine_mg_per_100ml(0.0) is None
    assert caffeine_mg_per_100ml(2.5) is None
    with_caf = filter_row({**base, "caffeine_100g": "0.032"})
    assert with_caf and with_caf["caffeine_mg_per_100ml"] == 32.0, with_caf
    merged = dedupe([e, {**e, "image_url": None}, {**e, "name": "Other"}])
    assert len(merged) == 2 and merged[0]["image_url"], merged
    print(f"selftest OK ({len(vectors)} flavour vectors)")
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
