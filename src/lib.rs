//! # daily-luck
//!
//! Rust port of the **PCL2** and **PCLCE** daily-luck (今日人品) algorithms
//! from [`Zyx-2012/daily-luck`](https://github.com/Zyx-2012/daily-luck).
//!
//! The crate provides:
//!
//! * Two pure scoring functions [`pcl2_luck`] / [`pclce_luck`] that map an
//!   identifier string plus a date to a deterministic luck value in `0..=100`.
//! * Two pure identifier builders [`pcl2_identify_from_registry_values`] /
//!   [`pclce_identify_from_hardware`] that reproduce the exact identify
//!   strings shown by PCL2 and PCLCE, given their inputs.
//! * The low-level primitives the algorithms are built from:
//!   [`stable_hash_64`] (MeloongCore's 64-bit UTF-16 hash), [`djb2_hash_32`],
//!   [`dotnet_random_next_101`] (.NET `System.Random(seed).Next(0, 101)`,
//!   reimplemented from the .NET reference source), and [`round_even`]
//!   (banker's rounding).
//! * On Windows, two system-access helpers [`pcl2_identify`] / [`pclce_identify`]
//!   that read the registry / WMI. On non-Windows targets they return
//!   [`IdentifyError::NotSupported`].
//!
//! ## Provenance
//!
//! The pure functions were validated against three independent implementations:
//!
//! * The verbatim JavaScript from `app.js` in the original repository
//!   (run with Node).
//! * An independent Python reimplementation of `server.py`'s logic.
//! * Native .NET `new Random(seed).Next(0, 101)` (for the subtractive generator).
//!
//! The 173 cross-validated vectors are committed under
//! [`tests/vectors/reference.json`](tests/vectors/reference.json) and
//! [`tests/vectors/reference_py.json`](tests/vectors/reference_py.json).
//! The unit tests load these files via [`include_str!`] and assert that every
//! Rust output matches the original project exactly.
//!
//! ## Notes on day-of-year
//!
//! PCL2 uses `dayOfYear(date)` which constructs local-midnight `Date` objects.
//! In time zones with DST transitions the result can drift by ±1 on DST-change
//! days. The target audience is Chinese (CST, UTC+08:00, no DST), so this
//! crate computes the standard ordinal day-of-year — identical to the JS value
//! for all DST-free environments.

#![forbid(unsafe_code)]

#[cfg(windows)]
use std::collections::HashMap;

#[cfg(windows)]
use wmi::{COMLibrary, Variant, WMIConnection};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// MeloongCore `GetStableHashCode` final XOR (64-bit).
const HASH_XOR: u64 = 0xA98F_501B_C684_032F;

/// .NET `Random` modulus: `Int32.MaxValue`.
const MBIG: i32 = 2_147_483_647;

/// .NET `Random` subtractive-generator seed constant.
const MSEED: i32 = 161_803_398;

// ---------------------------------------------------------------------------
// Pure hash helpers
// ---------------------------------------------------------------------------

/// MeloongCore's 64-bit stable hash over UTF-16 code units.
///
/// Mirrors `app.js`'s `stableHash(value)`: iterate the string's UTF-16 code
/// units, applying `result = ((result << 5) ^ result ^ char) & ((1<<64) - 1)`,
/// then XOR with `0xA98F501BC684032F`.
///
/// All shifts / additions use [`u64::wrapping_shl`] which yields the same low
/// 64 bits as the BigInt mask in the JS reference.
pub fn stable_hash_64(value: &str) -> u64 {
    let mut result: u64 = 5381;
    for unit in value.encode_utf16() {
        // ((result << 5) ^ result ^ char) & MASK_64  (see module note on wrapping)
        result = result.wrapping_shl(5) ^ result ^ u64::from(unit);
    }
    result ^ HASH_XOR
}

/// 32-bit DJB2-style hash over UTF-16 code units.
///
/// Mirrors `app.js`'s `djb2Hash(value)`: `hash = (hash * 33 + char) % 2**32`,
/// then final `% 2**31` — i.e. mask with `0x7FFF_FFFF`.
pub fn djb2_hash_32(value: &str) -> u32 {
    let mut hash: u32 = 5381;
    for unit in value.encode_utf16() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(unit));
    }
    hash & 0x7FFF_FFFF
}

/// Banker's rounding (round-half-to-even).
///
/// Mirrors `app.js`'s `roundEven(value)` exactly. Returns a `f64` holding an
/// exact integer, matching the JS Number semantics so the same value can
/// continue through the scoring pipeline's floating-point chain.
#[inline]
pub fn round_even(value: f64) -> f64 {
    let lower = value.floor();
    let fraction = value - lower;
    if fraction < 0.5 {
        lower
    } else if fraction > 0.5 {
        lower + 1.0
    } else if lower % 2.0 == 0.0 {
        lower
    } else {
        lower + 1.0
    }
}

// ---------------------------------------------------------------------------
// Allocation-free streamed hashing
//
// The scoring functions concatenate seeds with `format!` and then hash them.
// For the heavy workloads (year tables, perfect-day searches, random
// identifier scans) those temporary `String`s dominate the cost. The helpers
// below feed the exact same UTF-16 code-unit sequence directly into the hash,
// byte-for-byte identical to `format!` + [`stable_hash_64`] / [`djb2_hash_32`]
// (the streamed-vs-format equivalence is asserted in the unit tests), while
// performing zero heap allocations.
// ---------------------------------------------------------------------------

/// One step of the MeloongCore stable hash over one UTF-16 code unit.
#[inline(always)]
fn hash_step(result: u64, unit: u16) -> u64 {
    result.wrapping_shl(5) ^ result ^ u64::from(unit)
}

/// Feed ASCII bytes; each byte is its own UTF-16 code unit.
fn hash_ascii(mut result: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        result = hash_step(result, u16::from(b));
    }
    result
}

/// Feed the decimal representation of `n` — the same digits `Display` would
/// produce (no leading zeros, a leading `-` when negative).
fn hash_decimal(mut result: u64, n: i64) -> u64 {
    if n < 0 {
        result = hash_step(result, b'-' as u16);
    }
    let mut magnitude = n.unsigned_abs();
    let mut digits = [0u8; 20];
    let mut len = 0usize;
    loop {
        digits[len] = (magnitude % 10) as u8;
        len += 1;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }
    for &d in digits[..len].iter().rev() {
        result = hash_step(result, u16::from(b'0' + d));
    }
    result
}

/// Streamed `stable_hash_64` of the PCL2 first seed
/// `asdfgbn{doy}12#3$45{year}IUY` — no intermediate `String`.
fn hash_pcl2_first_seed(doy: u32, year: i32) -> u64 {
    let mut r = hash_ascii(5381, b"asdfgbn");
    r = hash_decimal(r, i64::from(doy));
    r = hash_ascii(r, b"12#3$45");
    r = hash_decimal(r, i64::from(year));
    r = hash_ascii(r, b"IUY");
    r ^ HASH_XOR
}

/// Streamed `stable_hash_64` of the PCL2 second seed
/// `QWERTY{identifier}0*8&6{day}kjhg` — no intermediate `String`.
fn hash_pcl2_second_seed(identifier: &str, day: u32) -> u64 {
    let mut r = hash_ascii(5381, b"QWERTY");
    for unit in identifier.encode_utf16() {
        r = hash_step(r, unit);
    }
    r = hash_ascii(r, b"0*8&6");
    r = hash_decimal(r, i64::from(day));
    r = hash_ascii(r, b"kjhg");
    r ^ HASH_XOR
}

/// One step of the DJB2 hash over one UTF-16 code unit.
#[inline(always)]
fn djb2_step(result: u32, unit: u16) -> u32 {
    result.wrapping_mul(33).wrapping_add(u32::from(unit))
}

/// Streamed `djb2_hash_32` of the PCLCE seed
/// `{year:04}{month:02}{day:02}{identifier}` — no intermediate `String`.
///
/// `{:04}` / `{:02}` pad to a *total* width (the sign consumes one slot when
/// `year` is negative), which this reproduces exactly.
fn djb2_hash_pclce(year: i32, month: u32, day: u32, identifier: &str) -> u32 {
    let mut h = 5381u32;
    if year < 0 {
        h = djb2_step(h, b'-' as u16);
    }
    let mut magnitude = year.unsigned_abs();
    let mut digits = [0u8; 10];
    let mut len = 0usize;
    loop {
        digits[len] = (magnitude % 10) as u8;
        len += 1;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }
    let pad = if year < 0 { 3 } else { 4 };
    for _ in len..pad {
        h = djb2_step(h, b'0' as u16);
    }
    for &d in digits[..len].iter().rev() {
        h = djb2_step(h, u16::from(b'0' + d));
    }
    h = djb2_step(h, u16::from(b'0' + (month / 10) as u8));
    h = djb2_step(h, u16::from(b'0' + (month % 10) as u8));
    h = djb2_step(h, u16::from(b'0' + (day / 10) as u8));
    h = djb2_step(h, u16::from(b'0' + (day % 10) as u8));
    for unit in identifier.encode_utf16() {
        h = djb2_step(h, unit);
    }
    h & 0x7FFF_FFFF
}

/// Ordinal day of year (1 = 1 January).
///
/// Pure calendar arithmetic; no time-zone dependency. In DST-free zones this
/// equals the JS `dayOfYear(date)` that builds `new Date(y, m-1, d)` local
/// midnights.
pub fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    debug_assert!((1..=12).contains(&month), "month out of range");
    debug_assert!((1..=31).contains(&day), "day out of range");
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut doy = CUM[(month - 1) as usize] + day;
    if month > 2 && is_leap_year(year) {
        doy += 1;
    }
    doy
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ---------------------------------------------------------------------------
// .NET Random(seed).Next(0, 101)
// ---------------------------------------------------------------------------

/// Reimplementation of `System.Random(seed).Next(0, 101)` from the .NET
/// reference source.
///
/// The subtractive Knuth generator (`MSEED = 161803398`, 56-element seed
/// array, 4 mix passes) is reproduced verbatim. `Sample()` is called once
/// and `floor(sample / MBIG * 101)` yields the result — this matches both
/// the JS `dotnetRandomNext101` reference and native .NET for every tested
/// seed.
///
/// `seed == i32::MIN` is handled as in .NET (mapped to `i32::MAX` before
/// `abs`) so the call never panics on overflow.
pub fn dotnet_random_next_101(seed: i32) -> u32 {
    // .NET: subtraction = (Seed == Int32.MinValue) ? Int32.MaxValue : Math.Abs(Seed)
    let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };

    let mut seed_array = [0i32; 56];
    let mut mj: i32 = MSEED - subtraction;
    seed_array[55] = mj;
    let mut mk: i32 = 1;

    for index in 1..=54i32 {
        let ii = (21 * index) % 55;
        seed_array[ii as usize] = mk;
        mk = mj.wrapping_sub(mk);
        if mk < 0 {
            mk += MBIG;
        }
        mj = seed_array[ii as usize];
    }

    for _ in 1..=4 {
        for index in 1..=55i32 {
            let mut value = seed_array[index as usize]
                .wrapping_sub(seed_array[(1 + (index + 30) % 55) as usize]);
            if value < 0 {
                value += MBIG;
            }
            seed_array[index as usize] = value;
        }
    }

    // One Sample() call: inext = 0, inextp = 21.
    let loc_inext = 1usize; // 0 + 1
    let loc_inextp = 22usize; // 21 + 1
    let mut value = seed_array[loc_inext].wrapping_sub(seed_array[loc_inextp]);
    if value == MBIG {
        value -= 1;
    }
    if value < 0 {
        value += MBIG;
    }

    // Next(0, 101) = floor(Sample() * 101) — JS uses division first:
    //   Math.floor((internalSample() / mbig) * 101)
    // This matches native .NET for all tested seeds.
    let sample = (f64::from(value) / MBIG as f64) * 101.0;
    sample.floor() as u32
}

// ---------------------------------------------------------------------------
// Scoring functions
// ---------------------------------------------------------------------------

/// Compute the PCL2 daily-luck score for a given identifier and date.
///
/// Mirrors `app.js`'s `scoreForDate(date, identifier)`:
///
/// ```text
/// first_seed  = "asdfgbn" + dayOfYear + "12#3$45" + year + "IUY"
/// second_seed = "QWERTY" + identifier + "0*8&6" + day + "kjhg"
/// raw = abs((Number(stableHash(first_seed))/3 + Number(stableHash(second_seed))/3) / 527) % 1001
/// rounded = roundEven(raw)
/// score = rounded >= 970 ? 100 : roundEven((rounded/969)*99)
/// ```
///
/// Returns a value in `0..=100`. The floating-point chain uses `u64 as f64`
/// (round-to-nearest-even) to mirror JS `Number(BigInt)` and `f64::rem` for
/// `% 1001.0`, both of which match ECMAScript semantics exactly.
pub fn pcl2_luck(identifier: &str, year: i32, month: u32, day: u32) -> u32 {
    pcl2_luck_with_first_hash(identifier, year, month, day, pcl2_first_hash(year, month, day))
}

/// Precompute the PCL2 first-seed hash for a date (`asdfgbn{doy}12#3$45{year}IUY`,
/// divided by 3). It depends only on the date, never on the identifier, so
/// callers scanning many identifiers over the same date range can compute it
/// once per day and reuse it — see [`pcl2_luck_with_first_hash`].
pub fn pcl2_first_hash(year: i32, month: u32, day: u32) -> f64 {
    let doy = day_of_year(year, month, day);
    (hash_pcl2_first_seed(doy, year) as f64) / 3.0
}

/// Compute the PCL2 daily-luck score reusing a first-seed hash precomputed by
/// [`pcl2_first_hash`] for the same date. Produces the exact same result as
/// [`pcl2_luck`].
pub fn pcl2_luck_with_first_hash(
    identifier: &str,
    year: i32,
    month: u32,
    day: u32,
    first_hash: f64,
) -> u32 {
    // `year`/`month` only influence the first seed, which the caller already
    // precomputed via `pcl2_first_hash`; keep them in the signature so the
    // date is fully described at the call site.
    let _ = (year, month);

    let second_hash = (hash_pcl2_second_seed(identifier, day) as f64) / 3.0;

    let raw = ((first_hash + second_hash).abs() / 527.0) % 1001.0;
    let rounded = round_even(raw);

    if rounded >= 970.0 {
        100
    } else {
        round_even((rounded / 969.0) * 99.0) as u32
    }
}

/// Compute the PCLCE daily-luck score for a given identifier and date.
///
/// Mirrors `app.js`'s `pclceScoreForDate(date, identifier)`:
///
/// ```text
/// seed = DJB2Hash(yyyyMMdd + identifier)   // 32-bit, & 0x7FFFFFFF
/// .NET Random(seed).Next(0, 101)
/// ```
///
/// Returns a value in `0..=100`.
pub fn pclce_luck(identifier: &str, year: i32, month: u32, day: u32) -> u32 {
    let seed = djb2_hash_pclce(year, month, day, identifier) as i32;
    dotnet_random_next_101(seed)
}

// ---------------------------------------------------------------------------
// Identifier builders (pure, testable without OS access)
// ---------------------------------------------------------------------------

/// Compute the PCL2 identify string from the two registry values it is
/// composed of.
///
/// Mirrors `server.py`'s `format_pcl_identify`:
///
/// 1. Normalize `last_config`: uppercase, strip leading/trailing `{` and `}`.
/// 2. `value = stable_hash(normalized + identify_seed)`.
/// 3. Format `value` as 16 uppercase hex digits.
/// 4. Rearrange as `hex[4:8]-hex[12:16]-hex[0:4]-hex[8:12]`.
pub fn pcl2_identify_from_registry_values(last_config: &str, identify_seed: &str) -> String {
    let upper = last_config.to_uppercase();
    let normalized = upper.trim_matches(|c| c == '{' || c == '}');
    let value = stable_hash_64(&format!("{normalized}{identify_seed}"));
    let hex_value = format!("{value:016X}");
    format!(
        "{}-{}-{}-{}",
        &hex_value[4..8],
        &hex_value[12..16],
        &hex_value[0..4],
        &hex_value[8..12],
    )
}

/// Compute the PCLCE identify string from the four WMI fields.
///
/// Mirrors `server.py`'s `format_pclce_identify`:
///
/// 1. Concatenate as `UUID:{u}|MB_Prod:{p}|MB_SN:{s}|CPU:{c}` (UTF-8).
/// 2. `raw_hash = sha512(raw)` (lowercase hex).
/// 3. `sample = sha512("PCL-CE|" + raw_hash + "|LauncherId")`.
/// 4. Take hex characters 64..80, uppercase, join as 4×4 groups with dashes.
pub fn pclce_identify_from_hardware(
    uuid: &str,
    mb_product: &str,
    mb_serial: &str,
    cpu_id: &str,
) -> String {
    use sha2::{Digest, Sha512};

    let raw = format!("UUID:{uuid}|MB_Prod:{mb_product}|MB_SN:{mb_serial}|CPU:{cpu_id}");
    let raw_hash = hex::encode(Sha512::digest(raw.as_bytes()));
    let sample_input = format!("PCL-CE|{raw_hash}|LauncherId");
    let sample = hex::encode(Sha512::digest(sample_input.as_bytes()));
    let hex_value = sample[64..80].to_uppercase();
    format!(
        "{}-{}-{}-{}",
        &hex_value[0..4],
        &hex_value[4..8],
        &hex_value[8..12],
        &hex_value[12..16],
    )
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error returned by [`pcl2_identify`] / [`pclce_identify`].
#[derive(Debug)]
pub enum IdentifyError {
    /// The current platform does not support this lookup (non-Windows).
    NotSupported(&'static str),
    /// WMI / COM layer reported an error.
    #[cfg_attr(not(windows), allow(dead_code))]
    Wmi(String),
    /// The required data was not present (PCL2/PCLCE never installed,
    /// or all WMI fields empty).
    MissingData(String),
}

impl std::fmt::Display for IdentifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotSupported(msg) => write!(f, "not supported: {msg}"),
            Self::Wmi(msg) => write!(f, "WMI error: {msg}"),
            Self::MissingData(msg) => write!(f, "missing data: {msg}"),
        }
    }
}

impl std::error::Error for IdentifyError {}

// ---------------------------------------------------------------------------
// System-access helpers (Windows-only)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn read_registry_string(parent: &winreg::RegKey, subpath: &str, name: &str) -> String {
    use winreg::enums::{REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ};

    let key = match parent.open_subkey(subpath) {
        Ok(k) => k,
        Err(_) => return String::new(),
    };
    if let Ok(s) = key.get_value::<String, _>(name) {
        return s;
    }
    let raw = match key.get_raw_value(name) {
        Ok(r) => r,
        Err(_) => return String::new(),
    };
    match raw.vtype {
        REG_SZ | REG_EXPAND_SZ | REG_MULTI_SZ => decode_utf16_string(&raw.bytes),
        REG_DWORD if raw.bytes.len() == 4 => {
            u32::from_le_bytes(raw.bytes[..4].try_into().unwrap()).to_string()
        }
        REG_QWORD if raw.bytes.len() == 8 => {
            u64::from_le_bytes(raw.bytes[..8].try_into().unwrap()).to_string()
        }
        _ => String::new(),
    }
}

#[cfg(windows)]
fn decode_utf16_string(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let unit = u16::from_le_bytes([chunk[0], chunk[1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    // `from_utf16_lossy` decodes surrogate pairs correctly (CJK Extension-B
    // chars, emoji, …); a lone surrogate becomes U+FFFD, the same fallback the
    // previous per-code-unit `char::from_u32` produced.
    String::from_utf16_lossy(&units)
}

/// Read the PCL2 identify from `HKCU\\Software\\PCL\\Identify` (or the
/// `PCLDebug` branch) combined with `HKLM\\SYSTEM\\HardwareConfig\\LastConfig`.
///
/// Mirrors `server.py`'s `read_pcl_identify()`. Returns [`IdentifyError::MissingData`]
/// if neither PCL2 branch has a usable seed.
#[cfg(windows)]
pub fn pcl2_identify() -> Result<String, IdentifyError> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    for folder in ["PCL", "PCLDebug"] {
        let identify_seed = read_registry_string(
            &hkcu,
            &format!(r"Software\{folder}"),
            "Identify",
        );
        let last_config = read_registry_string(
            &hklm,
            r"SYSTEM\HardwareConfig",
            "LastConfig",
        );
        // Python: `len(identify_seed) >= 3 and last_config`.
        if identify_seed.chars().count() >= 3 && !last_config.is_empty() {
            return Ok(pcl2_identify_from_registry_values(&last_config, &identify_seed));
        }
    }
    Err(IdentifyError::MissingData(
        "PCL2 Identify data was not found. Launch PCL2 once, then reload this page.".into(),
    ))
}

/// Read the PCLCE identify from WMI (`Win32_ComputerSystemProduct.UUID`,
/// `Win32_BaseBoard.Product`, `Win32_BaseBoard.SerialNumber`,
/// `Win32_Processor.ProcessorId`).
///
/// Mirrors `server.py`'s `read_pclce_hardware()` + `format_pclce_identify()`.
/// The four WMI properties are read with the `wmi` crate; the first present
/// (non-null) value per class is used, mirroring PowerShell's
/// `Get-FirstWmiValue`.
#[cfg(windows)]
pub fn pclce_identify() -> Result<String, IdentifyError> {
    let com = COMLibrary::new().map_err(|e| IdentifyError::Wmi(format!("COM init failed: {e}")))?;
    let conn = WMIConnection::new(com)
        .map_err(|e| IdentifyError::Wmi(format!("WMI connect failed: {e}")))?;

    let uuid = first_wmi_value(&conn, "Win32_ComputerSystemProduct", "UUID")?;
    let mb_prod = first_wmi_value(&conn, "Win32_BaseBoard", "Product")?;
    let mb_sn = first_wmi_value(&conn, "Win32_BaseBoard", "SerialNumber")?;
    let cpu = first_wmi_value(&conn, "Win32_Processor", "ProcessorId")?;

    // Python `not any(hardware.values())` → unavailable.
    if uuid.is_empty() && mb_prod.is_empty() && mb_sn.is_empty() && cpu.is_empty() {
        return Err(IdentifyError::MissingData(
            "PCLCE hardware identification was not available.".into(),
        ));
    }

    Ok(pclce_identify_from_hardware(&uuid, &mb_prod, &mb_sn, &cpu))
}

#[cfg(windows)]
fn first_wmi_value(
    conn: &WMIConnection,
    class: &str,
    prop: &str,
) -> Result<String, IdentifyError> {
    let query = format!("SELECT {prop} FROM {class}");
    let rows: Vec<HashMap<String, Variant>> = conn
        .raw_query(query)
        .map_err(|e| IdentifyError::Wmi(format!("{class}.{prop} query failed: {e}")))?;
    for row in rows {
        if let Some(v) = row.get(prop) {
            if let Some(s) = variant_to_string(v) {
                return Ok(s);
            }
        }
    }
    Ok(String::new())
}

#[cfg(windows)]
fn variant_to_string(v: &Variant) -> Option<String> {
    match v {
        // Mirror PowerShell's `($null -ne $value).Trim()`: skip Null/Empty,
        // accept String (even empty) after trimming whitespace.
        Variant::String(s) => Some(s.trim().to_string()),
        Variant::Empty | Variant::Null => None,
        _ => None,
    }
}

#[cfg(not(windows))]
pub fn pcl2_identify() -> Result<String, IdentifyError> {
    Err(IdentifyError::NotSupported(
        "PCL2 registry lookup is only available on Windows.",
    ))
}

#[cfg(not(windows))]
pub fn pclce_identify() -> Result<String, IdentifyError> {
    Err(IdentifyError::NotSupported(
        "PCLCE WMI lookup is only available on Windows.",
    ))
}

// ===========================================================================
// Unit tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    // -----------------------------------------------------------------------
    // Reference vectors (committed fixtures generated by tools/gen_ref.js
    // and tools/gen_ref.py from the original project's algorithms).
    // -----------------------------------------------------------------------

    const REF_JS: &str = include_str!("../tests/vectors/reference.json");
    const REF_PY: &str = include_str!("../tests/vectors/reference_py.json");

    fn js() -> Value { serde_json::from_str(REF_JS).unwrap() }
    fn py() -> Value { serde_json::from_str(REF_PY).unwrap() }

    // -----------------------------------------------------------------------
    // Hash primitives
    // -----------------------------------------------------------------------

    #[test]
    fn stable_hash_64_matches_reference() {
        let js = js();
        for entry in js["stableHash"].as_array().unwrap() {
            let s = entry["s"].as_str().unwrap();
            let expected_hex = entry["hex"].as_str().unwrap(); // "0x...016X"
            let expected_dec = entry["dec"].as_str().unwrap();
            let expected_f64 = entry["f64"].as_f64().unwrap();

            let got = stable_hash_64(s);
            let got_hex = format!("0x{got:016X}");
            let got_dec = got.to_string();
            let got_f64 = got as f64;

            assert_eq!(got_hex, expected_hex, "hex mismatch for {s:?}");
            assert_eq!(got_dec, expected_dec, "dec mismatch for {s:?}");
            // Both sides are the same IEEE-754 double; compare bit-for-bit.
            assert_eq!(got_f64.to_bits(), expected_f64.to_bits(),
                "f64 bits differ for {s:?}: got {got_f64}, expected {expected_f64}");
        }
    }

    #[test]
    fn djb2_hash_32_matches_reference() {
        let js = js();
        for entry in js["djb2"].as_array().unwrap() {
            let s = entry["s"].as_str().unwrap();
            let expected = entry["v"].as_u64().unwrap() as u32;
            assert_eq!(djb2_hash_32(s), expected, "djb2 mismatch for {s:?}");
        }
    }

    #[test]
    fn dotnet_random_next_101_matches_reference() {
        let js = js();
        for entry in js["random101"].as_array().unwrap() {
            let seed = entry["seed"].as_i64().unwrap() as i32;
            let expected = entry["v"].as_u64().unwrap() as u32;
            assert_eq!(
                dotnet_random_next_101(seed), expected,
                "random101 mismatch for seed {seed}",
            );
        }
    }

    #[test]
    fn round_even_matches_reference() {
        let js = js();
        for entry in js["roundEven"].as_array().unwrap() {
            let v = entry["v"].as_f64().unwrap();
            let expected = entry["r"].as_f64().unwrap();
            let got = round_even(v);
            assert_eq!(got.to_bits(), expected.to_bits(),
                "roundEven mismatch for v={v}: got {got}, expected {expected}");
        }
    }

    #[test]
    fn round_even_corner_cases() {
        // Tie → even.
        assert_eq!(round_even(0.5), 0.0);
        assert_eq!(round_even(1.5), 2.0);
        assert_eq!(round_even(2.5), 2.0);
        assert_eq!(round_even(3.5), 4.0);
        assert_eq!(round_even(4.5), 4.0);
        assert_eq!(round_even(1000.5), 1000.0);
        assert_eq!(round_even(969.5), 970.0);
        // Strict rounding.
        assert_eq!(round_even(0.4999999), 0.0);
        assert_eq!(round_even(0.5000001), 1.0);
        assert_eq!(round_even(42.7), 43.0);
    }

    #[test]
    fn day_of_year_known_dates() {
        assert_eq!(day_of_year(2024, 1, 1), 1);
        assert_eq!(day_of_year(2024, 2, 29), 60);
        assert_eq!(day_of_year(2024, 3, 1), 61);
        assert_eq!(day_of_year(2024, 12, 31), 366);
        assert_eq!(day_of_year(2023, 12, 31), 365);
        assert_eq!(day_of_year(2000, 2, 29), 60);   // leap
        assert_eq!(day_of_year(1900, 3, 1), 60);    // non-leap century
        assert_eq!(day_of_year(2100, 2, 28), 59);   // non-leap century
        assert_eq!(day_of_year(2400, 12, 31), 366); // leap quad-century
    }

    // -----------------------------------------------------------------------
    // Streamed hashing vs. `format!`-based reference (bit-for-bit identity)
    // -----------------------------------------------------------------------

    #[test]
    fn streamed_pcl2_first_seed_matches_format_reference() {
        // The allocation-free streamed hash must equal `stable_hash_64` over
        // the string `format!` would have built, for every input shape.
        for doy in [1u32, 2, 9, 10, 59, 60, 100, 365, 366] {
            for year in [1i32, 999, 1000, 1999, 2024, 2025, 2100] {
                let streamed = hash_pcl2_first_seed(doy, year);
                let reference = stable_hash_64(&format!("asdfgbn{doy}12#3$45{year}IUY"));
                assert_eq!(streamed, reference, "first seed mismatch doy={doy} year={year}");
            }
        }
        // Negative years (format! emits a leading '-'; width not used here).
        for year in [-1i32, -5, -2025] {
            let streamed = hash_pcl2_first_seed(100, year);
            let reference = stable_hash_64(&format!("asdfgbn10012#3$45{year}IUY"));
            assert_eq!(streamed, reference, "first seed mismatch year={year}");
        }
    }

    #[test]
    fn streamed_pcl2_second_seed_matches_format_reference() {
        for id in [
            "",
            "WEB",
            "WEB-123456",
            "ABCD-EFGH-1234-5678",
            "cafe-babe-dead-beef",
            "中文标识",
            "𝄞music🎵",
        ] {
            for day in [1u32, 9, 10, 31] {
                let streamed = hash_pcl2_second_seed(id, day);
                let reference = stable_hash_64(&format!("QWERTY{id}0*8&6{day}kjhg"));
                assert_eq!(streamed, reference, "second seed mismatch id={id:?} day={day}");
            }
        }
    }

    #[test]
    fn streamed_pclce_seed_matches_format_reference() {
        for year in [1i32, 999, 1000, 2024, 2025, 2100, -5, -2025] {
            for month in [1u32, 2, 9, 10, 12] {
                for day in [1u32, 5, 9, 10, 31] {
                    for id in ["", "WEB", "ABCD-EFGH-1234-5678", "中文"] {
                        let streamed = djb2_hash_pclce(year, month, day, id);
                        let reference =
                            djb2_hash_32(&format!("{year:04}{month:02}{day:02}{id}"));
                        assert_eq!(
                            streamed, reference,
                            "pclce seed mismatch {year}-{month}-{day} id={id:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pcl2_luck_with_precomputed_first_hash_matches() {
        // The split API must produce the same score as the plain call.
        for (id, y, m, d) in [
            ("ABCD-EFGH-1234-5678", 2025, 1, 1),
            ("中文标识", 2024, 2, 29),
            ("cafe-babe-dead-beef", 2023, 6, 15),
            ("WEB-123456", 2100, 12, 31),
            ("", 2025, 3, 1),
        ] {
            let direct = pcl2_luck(id, y, m, d);
            let first = pcl2_first_hash(y, m, d);
            let split = pcl2_luck_with_first_hash(id, y, m, d, first);
            assert_eq!(direct, split, "split mismatch id={id:?} {y}-{m}-{d}");
        }
    }

    // -----------------------------------------------------------------------
    // Performance check (manual): cargo test --release --lib -- --ignored bench
    // -----------------------------------------------------------------------

    #[test]
    #[ignore = "manual perf check: cargo test --release --lib -- --ignored bench_pcl2"]
    fn bench_pcl2_luck_optimized_vs_format_reference() {
        use std::time::Instant;
        const N: usize = 200_000;
        let ids: Vec<String> = (0..100)
            .map(|i| format!("{:04X}-{:04X}-{:04X}-{:04X}", i, i * 2, i * 3, i * 4))
            .collect();

        // Optimized path (streamed, allocation-free).
        let start = Instant::now();
        let mut acc = 0u32;
        for i in 0..N {
            let id = &ids[i % ids.len()];
            let (y, m, d) = (2025 + (i % 3) as i32, 1 + (i % 12) as u32, 1 + (i % 28) as u32);
            acc = acc.wrapping_add(pcl2_luck(id, y, m, d));
        }
        let optimized = start.elapsed();

        // Reference path (the old `format!`-based implementation, inlined).
        let start = Instant::now();
        let mut acc_ref = 0u32;
        for i in 0..N {
            let id = &ids[i % ids.len()];
            let (y, m, d) = (2025 + (i % 3) as i32, 1 + (i % 12) as u32, 1 + (i % 28) as u32);
            let doy = day_of_year(y, m, d);
            let first_seed = format!("asdfgbn{doy}12#3$45{y}IUY");
            let second_seed = format!("QWERTY{id}0*8&6{d}kjhg");
            let first_hash = (stable_hash_64(&first_seed) as f64) / 3.0;
            let second_hash = (stable_hash_64(&second_seed) as f64) / 3.0;
            let raw = ((first_hash + second_hash).abs() / 527.0) % 1001.0;
            let rounded = round_even(raw);
            let score = if rounded >= 970.0 {
                100
            } else {
                round_even((rounded / 969.0) * 99.0) as u32
            };
            acc_ref = acc_ref.wrapping_add(score);
        }
        let reference = start.elapsed();

        // Same results on both paths (in-release consistency proof).
        assert_eq!(acc, acc_ref, "optimized and reference results differ");
        eprintln!(
            "pcl2_luck bench N={N}: optimized={optimized:?} reference={reference:?} \
             speedup={:.2}x",
            reference.as_secs_f64() / optimized.as_secs_f64()
        );
    }

    #[cfg(windows)]
    #[test]
    fn decode_utf16_string_handles_surrogate_pairs() {
        let utf16le = |s: &str| -> Vec<u8> {
            let mut v = Vec::new();
            for unit in s.encode_utf16() {
                v.extend_from_slice(&unit.to_le_bytes());
            }
            v
        };
        // BMP chars, a surrogate pair (𝄞) and a trailing NUL terminator.
        let mut bytes = utf16le("A中𝄞");
        bytes.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(decode_utf16_string(&bytes), "A中𝄞");
        // Stops at the first NUL.
        let mut bytes2 = utf16le("ABC");
        bytes2.extend_from_slice(&0u16.to_le_bytes());
        bytes2.extend_from_slice(&utf16le("DEF"));
        assert_eq!(decode_utf16_string(&bytes2), "ABC");
        // A lone high surrogate becomes U+FFFD (same as the old code path).
        let mut bytes3 = vec![0x00u8, 0xD8u8];
        bytes3.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(decode_utf16_string(&bytes3), "\u{FFFD}");
    }

    // -----------------------------------------------------------------------
    // Full scoring functions
    // -----------------------------------------------------------------------

    fn parse_date(s: &str) -> (i32, u32, u32) {
        let mut parts = s.split('-').map(|p| p.parse::<u32>().unwrap());
        let y = parts.next().unwrap() as i32;
        let m = parts.next().unwrap();
        let d = parts.next().unwrap();
        (y, m, d)
    }

    #[test]
    fn pcl2_luck_matches_reference() {
        let js = js();
        for entry in js["pcl2"].as_array().unwrap() {
            let id = entry["identifier"].as_str().unwrap();
            let (y, m, d) = parse_date(entry["date"].as_str().unwrap());
            let expected = entry["score"].as_u64().unwrap() as u32;
            let got = pcl2_luck(id, y, m, d);
            assert_eq!(
                got, expected,
                "pcl2_luck mismatch for id={id:?} {y}-{m:02}-{d:02}",
            );
        }
    }

    #[test]
    fn pclce_luck_matches_reference() {
        let js = js();
        for entry in js["pclce"].as_array().unwrap() {
            let id = entry["identifier"].as_str().unwrap();
            let (y, m, d) = parse_date(entry["date"].as_str().unwrap());
            let expected = entry["score"].as_u64().unwrap() as u32;
            let got = pclce_luck(id, y, m, d);
            assert_eq!(
                got, expected,
                "pclce_luck mismatch for id={id:?} {y}-{m:02}-{d:02}",
            );
        }
    }

    // -----------------------------------------------------------------------
    // Identifier builders
    // -----------------------------------------------------------------------

    #[test]
    fn pcl2_identify_from_registry_values_matches_python() {
        let py = py();
        for entry in py["pclIdentify"].as_array().unwrap() {
            let last = entry["lastConfig"].as_str().unwrap();
            let seed = entry["seed"].as_str().unwrap();
            let expected = entry["identify"].as_str().unwrap();
            let got = pcl2_identify_from_registry_values(last, seed);
            assert_eq!(got, expected,
                "pcl2_identify mismatch for last={last:?}, seed={seed:?}");
        }
    }

    #[test]
    fn pclce_identify_from_hardware_matches_python() {
        let py = py();
        for entry in py["pclceIdentify"].as_array().unwrap() {
            let hw = &entry["hardware"];
            let expected = entry["identify"].as_str().unwrap();
            let got = pclce_identify_from_hardware(
                hw["UUID"].as_str().unwrap(),
                hw["MB_Prod"].as_str().unwrap(),
                hw["MB_SN"].as_str().unwrap(),
                hw["CPU"].as_str().unwrap(),
            );
            assert_eq!(got, expected,
                "pclce_identify mismatch for hardware {:?}", hw);
        }
    }

    #[test]
    fn pclce_sha512_trace_matches_python() {
        // Cross-check the SHA-512 intermediate values against the Python
        // reference (server.py hashlib).
        let py = py();
        let trace = &py["pclceTrace"];
        let raw = trace["raw"].as_str().unwrap();
        let expected_raw_hash = trace["rawHash"].as_str().unwrap();
        let expected_sample = trace["sample"].as_str().unwrap();
        let expected_hex_value = trace["sample64_80"].as_str().unwrap();

        use sha2::{Digest, Sha512};
        let raw_hash = hex::encode(Sha512::digest(raw.as_bytes()));
        assert_eq!(raw_hash, expected_raw_hash, "sha512(raw) mismatch");

        let sample_input = format!("PCL-CE|{raw_hash}|LauncherId");
        let sample = hex::encode(Sha512::digest(sample_input.as_bytes()));
        assert_eq!(sample, expected_sample, "sha512(sample input) mismatch");

        let hex_value = sample[64..80].to_uppercase();
        assert_eq!(hex_value, expected_hex_value);
    }

    // -----------------------------------------------------------------------
    // Score bounds (sanity)
    // -----------------------------------------------------------------------

    #[test]
    fn pcl2_luck_is_within_0_to_100() {
        for year in 2020..=2026 {
            for month in 1..=12u32 {
                let day_max = [31, if is_leap_year(year) { 29 } else { 28 },
                               31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month as usize - 1];
                for day in 1..=day_max {
                    let s = pcl2_luck("test-identifier", year, month, day);
                    assert!(s <= 100, "pcl2_luck out of range: {s} for {year}-{month}-{day}");
                }
            }
        }
    }

    #[test]
    fn pclce_luck_is_within_0_to_100() {
        for year in 2020..=2026 {
            for month in 1..=12u32 {
                let day_max = [31, if is_leap_year(year) { 29 } else { 28 },
                               31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month as usize - 1];
                for day in 1..=day_max {
                    let s = pclce_luck("test-identifier", year, month, day);
                    assert!(s <= 100, "pclce_luck out of range: {s} for {year}-{month}-{day}");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Live system tests (only run on Windows, ignored by default).
    //
    // These exercise the registry / WMI lookup paths and simply assert that
    // the returned identify strings have the expected 19-character shape
    // "XXXX-XXXX-XXXX-XXXX". For cross-validation against Python's live
    // output, run `python server.py` on the same machine and compare the
    // `/api/identifiers` response against these tests' printed values.
    // -----------------------------------------------------------------------

    #[cfg(windows)]
    #[test]
    #[ignore = "requires PCL2 to have been launched at least once on this machine"]
    fn live_pcl2_identify() {
        match pcl2_identify() {
            Ok(id) => {
                eprintln!("[live] pcl2 identify = {id}");
                assert!(
                    id.split('-').all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit())),
                    "identify has unexpected shape: {id:?}"
                );
            }
            Err(IdentifyError::MissingData(msg)) => {
                eprintln!("[live] pcl2 missing: {msg}");
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires WMI; run manually to cross-validate with server.py"]
    fn live_pclce_identify() {
        match pclce_identify() {
            Ok(id) => {
                eprintln!("[live] pclce identify = {id}");
                assert!(
                    id.split('-').all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit())),
                    "identify has unexpected shape: {id:?}"
                );
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
}
