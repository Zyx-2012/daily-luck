#!/usr/bin/env python3
"""Reference vector generator (independent Python reimplementation).

Reimplements the pure functions of the original project
https://github.com/Zyx-2012/daily-luck (server.py + app.js) from scratch, so it
can cross-check the Node vectors produced by tools/gen_ref.js.

    python tools/gen_ref.py [output.json]

Python ints/floats are IEEE-754 doubles, and int->float conversion uses
round-to-nearest-even, matching JS Number(BigInt) and Rust (u64 as f64).
"""
import hashlib
import json
import math
import sys

MASK_64 = (1 << 64) - 1
HASH_XOR = 0xA98F501BC684032F


def utf16_units(value):
    encoded = value.encode("utf-16-le")
    return [int.from_bytes(encoded[i:i + 2], "little") for i in range(0, len(encoded), 2)]


def stable_hash(value):
    """server.py stable_hash: MeloongCore 64-bit UTF-16 stable hash."""
    result = 5381
    for c in utf16_units(value):
        result = ((result << 5) ^ result ^ c) & MASK_64
    return result ^ HASH_XOR


def djb2_hash(value):
    hash_ = 5381
    for c in utf16_units(value):
        hash_ = (hash_ * 33 + c) % 0x100000000
    return hash_ % 0x80000000


def dotnet_random_next_101(seed):
    """JS dotnetRandomNext101 (app.js): mirrors .NET Random(seed).Next(0, 101)."""
    mbig = 2147483647
    mseed = 161803398
    seed_array = [0] * 56
    mj = mseed - abs(seed)
    seed_array[55] = mj
    mk = 1
    for index in range(1, 55):
        ii = (21 * index) % 55
        seed_array[ii] = mk
        mk = mj - mk
        if mk < 0:
            mk += mbig
        mj = seed_array[ii]
    for _ in range(4):
        for index in range(1, 56):
            value = seed_array[index] - seed_array[1 + ((index + 30) % 55)]
            if value < 0:
                value += mbig
            seed_array[index] = value
    inext = 0
    inextp = 21

    def internal_sample():
        nonlocal inext, inextp
        loc_inext = inext + 1
        if loc_inext >= 56:
            loc_inext = 1
        loc_inextp = inextp + 1
        if loc_inextp >= 56:
            loc_inextp = 1
        value = seed_array[loc_inext] - seed_array[loc_inextp]
        if value == mbig:
            value -= 1
        if value < 0:
            value += mbig
        seed_array[loc_inext] = value
        inext = loc_inext
        inextp = loc_inextp
        return value

    return math.floor((internal_sample() / mbig) * 101)


def round_even(value):
    lower = math.floor(value)
    fraction = value - lower
    if fraction < 0.5:
        return lower
    if fraction > 0.5:
        return lower + 1
    return lower if lower % 2 == 0 else lower + 1


def day_of_year(year, month, day):
    import datetime
    return datetime.date(year, month, day).timetuple().tm_yday


def pcl2_score(identifier, year, month, day):
    doy = day_of_year(year, month, day)
    first_seed = f"asdfgbn{doy}12#3$45{year}IUY"
    second_seed = f"QWERTY{identifier}0*8&6{day}kjhg"
    first_hash = stable_hash(first_seed) / 3.0
    second_hash = stable_hash(second_seed) / 3.0
    raw = abs((first_hash + second_hash) / 527.0) % 1001.0
    rounded = round_even(raw)
    return 100 if rounded >= 970 else round_even((rounded / 969.0) * 99.0)


def pclce_score(identifier, year, month, day):
    date_part = f"{year:04d}{month:02d}{day:02d}"
    return dotnet_random_next_101(djb2_hash(date_part + identifier))


def format_pcl_identify(last_config, identify_seed):
    """server.py format_pcl_identify."""
    normalized_config = last_config.upper().strip("{}")
    value = stable_hash(normalized_config + identify_seed)
    hex_value = f"{value:016X}"
    return "-".join((hex_value[4:8], hex_value[12:16], hex_value[0:4], hex_value[8:12]))


def format_pclce_identify(hardware):
    """server.py format_pclce_identify."""
    raw = (
        f"UUID:{hardware['UUID']}"
        f"|MB_Prod:{hardware['MB_Prod']}"
        f"|MB_SN:{hardware['MB_SN']}"
        f"|CPU:{hardware['CPU']}"
    )
    raw_hash = hashlib.sha512(raw.encode("utf-8")).hexdigest()
    sample = hashlib.sha512(f"PCL-CE|{raw_hash}|LauncherId".encode("utf-8")).hexdigest()
    hex_value = sample[64:80].upper()
    return "-".join((hex_value[0:4], hex_value[4:8], hex_value[8:12], hex_value[12:16]))


def main():
    out = {}

    hash_strings = [
        "",
        "a",
        "hello world",
        "asdfgbn112#3$452024IUY",
        "QWERTYABCD-EFGH-1234-56780*8&66kjhg",
        "PCL",
        "{D0FCA2E4-6C10-4D7A-9C5B-123456789ABC}",
        "{abcdefgh-1234-5678-90ab-cdef12345678}",
        "中文标识测试",
        "𝄞music🎵",
        "PCL-CE|" + "a" * 128 + "|LauncherId",
        "x" * 300,
    ]
    out["stableHash"] = [
        {"s": s, "hex": f"0x{stable_hash(s):016X}", "dec": str(stable_hash(s)), "f64": float(stable_hash(s))}
        for s in hash_strings
    ]

    djb2_strings = [
        "",
        "20240101",
        "20240101WEB-123456",
        "20240229ABCD-EFGH-1234-5678",
        "PCLCE-test",
        "🀄🀄",
    ]
    out["djb2"] = [{"s": s, "v": djb2_hash(s)} for s in djb2_strings]

    random_seeds = [0, 1, 2, 42, 5381, 55555, 123456789, 987654321, 2147483647, 20240101, 999999999, 1190023354]
    out["random101"] = [{"seed": seed, "v": dotnet_random_next_101(seed)} for seed in random_seeds]

    round_values = [0.5, 1.5, 2.5, 3.5, 4.5, 0.0, 1.0, 0.4999999, 0.5000001, 1000.5, 969.5, 42.7, 969.0, 1000.9999, 970.0]
    out["roundEven"] = [{"v": v, "r": round_even(v)} for v in round_values]

    identifiers = [
        "",
        "WEB",
        "WEB-123456",
        "ABCD-EFGH-1234-5678",
        "cafe-babe-dead-beef",
        "test",
        "1234567890123456",
        "中文标识",
    ]
    dates = [
        (2024, 1, 1),
        (2024, 2, 29),
        (2024, 12, 31),
        (2025, 1, 1),
        (2000, 2, 29),
        (2023, 6, 15),
        (2100, 2, 28),
        (1999, 7, 7),
    ]

    out["pcl2"] = []
    out["pclce"] = []
    for identifier in identifiers:
        for (y, m, d) in dates:
            ds = f"{y:04d}-{m:02d}-{d:02d}"
            out["pcl2"].append({"identifier": identifier, "date": ds, "score": pcl2_score(identifier, y, m, d)})
            out["pclce"].append({"identifier": identifier, "date": ds, "score": pclce_score(identifier, y, m, d)})

    registry_pairs = [
        ("{D0FCA2E4-6C10-4D7A-9C5B-123456789ABC}", "aBc123"),
        ("{abcdefgh-1234-5678-90ab-cdef12345678}", "PCL2-Seed-42"),
        ("no-braces-config", "seed"),
        ("{{double}}", ""),
        ("{lowercase-guid}", "SeEd"),
        ("{}", "x"),
        ("", "abc"),
        ("{ABCD}", "  spaced seed  "),
    ]
    out["pclIdentify"] = [{"lastConfig": a, "seed": b, "identify": format_pcl_identify(a, b)} for (a, b) in registry_pairs]

    hardware_sets = [
        {"UUID": "11111111-2222-3333-4444-555555555555", "MB_Prod": "Standard", "MB_SN": "Default string", "CPU": "BFEBFBFF000906EA"},
        {"UUID": "", "MB_Prod": "", "MB_SN": "", "CPU": ""},
        {"UUID": "A", "MB_Prod": "B", "MB_SN": "C", "CPU": "D"},
        {"UUID": "UUID-中文", "MB_Prod": "主板", "MB_SN": "SN-123  ", "CPU": "CPU-0F"},
        {"UUID": "0A1B2C3D-4E5F-6789-0ABC-DEF012345678", "MB_Prod": "To be filled by O.E.M.", "MB_SN": "Default string", "CPU": "00000000"},
    ]
    out["pclceIdentify"] = [
        {"hardware": h, "identify": format_pclce_identify(h), "raw": None}
        for h in hardware_sets
    ]
    # intermediate sha512 trace for the first hardware set
    h0 = hardware_sets[0]
    raw0 = f"UUID:{h0['UUID']}|MB_Prod:{h0['MB_Prod']}|MB_SN:{h0['MB_SN']}|CPU:{h0['CPU']}"
    raw_hash0 = hashlib.sha512(raw0.encode("utf-8")).hexdigest()
    sample0 = hashlib.sha512(f"PCL-CE|{raw_hash0}|LauncherId".encode("utf-8")).hexdigest()
    out["pclceTrace"] = {"raw": raw0, "rawHash": raw_hash0, "sample": sample0, "sample64_80": sample0[64:80].upper()}

    target = sys.argv[1] if len(sys.argv) > 1 else "tests/vectors/reference_py.json"
    with open(target, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(out, fh, indent=2, ensure_ascii=True)
        fh.write("\n")
    print(f"wrote {target}", file=sys.stderr)


if __name__ == "__main__":
    main()