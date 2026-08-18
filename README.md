# Daily Luck PCL2/PCLCE Algorithm 🎲

Rust port of the **PCL2** (Plain Craft Launcher 2) and **PCLCE** daily-luck (今日人品) algorithms from [Zyx-2012/daily-luck](https://github.com/Zyx-2012/daily-luck).

## Features

- ✅ PCL2 Algorithm — Registry-based identify + GetStableHashCode 64-bit
- ✅ PCLCE Algorithm — WMI query + SHA-512 + DJB2 + .NET Random recreation
- ✅ Cross-platform pure functions (no OS dependencies)
- ✅ Windows system access (registry/WMI auto-detection)
- ✅ Desktop GUI application (egui/eframe) with card-based layout and system CJK font support
- ✅ Startup auto-computes 365-day forecast, shown in a pop-up window on demand
- ✅ Countdown to the next perfect (100) day within 1000 days + its date
- ✅ Identifier search (custom start date, day range and identifier count; 6 sort modes; runs on a background thread with a progress bar and paging)
- ✅ Developer settings (confirm-gated): lift day/count limits, checkpoint/resume caching to the user cache dir, load & delete cache
- ✅ Cross-compiled release builds for Windows x64

## Provenance & Verification

All algorithms were validated against three independent implementations:

| Implementation | Status | Notes |
|---------------|--------|-------|
| JavaScript (`app.js`) | ✅ Verified | Verbatim copy of original repository logic |
| Python (`server.py`) | ✅ Verified | Independent reimplementation of registry/WMI logic |
| Native .NET Reference | ✅ Verified | `new Random(seed).Next(0, 101)` matches our reconstruction |

The 173 cross-validated test vectors are committed under `tests/vectors/`.

## Quick Start

```bash
cargo run --release
```

## Project Structure

```
src/
├── lib.rs      # Pure algorithm library (PCL2, PCLCE scoring, identifiers)
└── main.rs     # egui desktop application entry point
tests/vectors/  # Reference JSON fixtures for unit tests
tools/          # Vector generation scripts (JS + Python)
.github/workflows/release.yml  # GitHub Actions for cross-compilation
```

## API Overview

### Pure Functions (Cross-Platform)

```rust
use daily_luck::{pcl2_luck, pclce_luck};

// Input: device identifier + date
let score = pcl2_luck("ABCD-EFGH-1234-5678", 2025, 1, 15);  // → 0..=100
let score = pclce_luck("WXYZ-1234-ABCD-5678", 2025, 1, 15); // → 0..=100
```

Both scorers are allocation-free: seeds are hashed streamed (no `format!`
temporaries), producing bit-for-bit identical results to the reference
implementation (~2.6× faster in release benchmarks).

For workloads that scan many identifiers over the same date range, the first
seed of the PCL2 algorithm depends only on the date:

```rust
let first = daily_luck::pcl2_first_hash(2025, 1, 15);
let score = daily_luck::pcl2_luck_with_first_hash("ABCD-EFGH-1234-5678", 2025, 1, 15, first);
// identical to pcl2_luck(...), but the date hash is computed once per day
```

### System Access (Windows Only)

```rust
#[cfg(windows)]
{
    let pcl2_id = daily_luck::pcl2_identify()?;  // From HKCU\Software\PCL + HKLM\SYSTEM\HardwareConfig
    let pclce_id = daily_luck::pclce_identify()?; // From WMI: Win32_ComputerSystemProduct etc.
}
```

## Build Configuration

- **Release profile**: LTO, single codegen unit, strip binaries, opt-level=z, panic=abort
- **CI/CD**: GitHub Actions pushes tag `v*`, builds Windows x64 binary, creates Release artifact
- **Dependencies**: eframe (GUI), chrono, rand, sha2, hex, winreg (WMI via windows-rs crate)

## License

MIT
