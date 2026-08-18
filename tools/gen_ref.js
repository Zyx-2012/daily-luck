#!/usr/bin/env node
"use strict";
/**
 * Reference vector generator — PCL2 / PCLCE daily-luck (今日人品).
 *
 * The functions below are copied VERBATIM from the original project
 * https://github.com/Zyx-2012/daily-luck (app.js), so the emitted vectors are
 * ground truth produced by the original JavaScript algorithm. Run with Node:
 *
 *     node tools/gen_ref.js [output.json]
 */
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
const MASK_64 = (1n << 64n) - 1n;
const HASH_XOR = 0xa98f501bc684032fn;

// --- verbatim copies from app.js ---

function stableHash(value) {
  let result = 5381n;
  for (let index = 0; index < value.length; index += 1) {
    result = ((result << 5n) ^ result ^ BigInt(value.charCodeAt(index))) & MASK_64;
  }
  return result ^ HASH_XOR;
}

function roundEven(value) {
  const lower = Math.floor(value);
  const fraction = value - lower;
  if (fraction < 0.5) return lower;
  if (fraction > 0.5) return lower + 1;
  return lower % 2 === 0 ? lower : lower + 1;
}

function djb2Hash(value) {
  let hash = 5381;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 33 + value.charCodeAt(index)) % 0x100000000;
  }
  return hash % 0x80000000;
}

function dotnetRandomNext101(seed) {
  const mbig = 2147483647;
  const mseed = 161803398;
  const seedArray = new Array(56).fill(0);
  let mj = mseed - Math.abs(seed);
  seedArray[55] = mj;
  let mk = 1;

  for (let index = 1; index <= 54; index += 1) {
    const ii = (21 * index) % 55;
    seedArray[ii] = mk;
    mk = mj - mk;
    if (mk < 0) mk += mbig;
    mj = seedArray[ii];
  }

  for (let pass = 1; pass <= 4; pass += 1) {
    for (let index = 1; index <= 55; index += 1) {
      let value = seedArray[index] - seedArray[1 + ((index + 30) % 55)];
      if (value < 0) value += mbig;
      seedArray[index] = value;
    }
  }

  let inext = 0;
  let inextp = 21;
  const internalSample = () => {
    let locInext = inext + 1;
    if (locInext >= 56) locInext = 1;
    let locInextp = inextp + 1;
    if (locInextp >= 56) locInextp = 1;
    let value = seedArray[locInext] - seedArray[locInextp];
    if (value === mbig) value -= 1;
    if (value < 0) value += mbig;
    seedArray[locInext] = value;
    inext = locInext;
    inextp = locInextp;
    return value;
  };

  return Math.floor((internalSample() / mbig) * 101);
}

function dayOfYear(date) {
  const start = new Date(date.getFullYear(), 0, 1);
  const current = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  return Math.floor((current - start) / 86400000) + 1;
}

function scoreForDate(date, identifier) {
  const firstSeed = `asdfgbn${dayOfYear(date)}12#3$45${date.getFullYear()}IUY`;
  const secondSeed = `QWERTY${identifier}0*8&6${date.getDate()}kjhg`;
  const firstHash = Number(stableHash(firstSeed)) / 3;
  const secondHash = Number(stableHash(secondSeed)) / 3;
  const raw = Math.abs((firstHash + secondHash) / 527) % 1001;
  const rounded = roundEven(raw);
  return rounded >= 970 ? 100 : roundEven((rounded / 969) * 99);
}

function pclceScoreForDate(date, identifier) {
  const datePart = `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}`;
  return dotnetRandomNext101(djb2Hash(`${datePart}${identifier}`));
}

function pad(value) {
  return String(value).padStart(2, "0");
}

// --- driver ---

function makeDate(y, m, d) {
  // Same construction as app.js dateFromInput() (noon local keeps the
  // day-of-year arithmetic identical to the app's calculate() flow).
  return new Date(y, m - 1, d, 12, 0, 0, 0);
}

const SHA256_STRING = (s) => s; // noop placeholder, unused

const hashStrings = [
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
  "PCL-CE|" + "a".repeat(128) + "|LauncherId",
  "x".repeat(300),
];

const djb2Strings = [
  "",
  "20240101",
  "20240101WEB-123456",
  "20240229ABCD-EFGH-1234-5678",
  "PCLCE-test",
  "🀄🀄",
];

const randomSeeds = [0, 1, 2, 42, 5381, 55555, 123456789, 987654321, 2147483647, 20240101, 999999999, 1190023354];

const roundValues = [0.5, 1.5, 2.5, 3.5, 4.5, 0.0, 1.0, 0.4999999, 0.5000001, 1000.5, 969.5, 42.7, 969.0, 1000.9999, 970.0];

const identifiers = [
  "",
  "WEB",
  "WEB-123456",
  "ABCD-EFGH-1234-5678",
  "cafe-babe-dead-beef",
  "test",
  "1234567890123456",
  "中文标识",
];

const dates = [
  [2024, 1, 1],
  [2024, 2, 29],
  [2024, 12, 31],
  [2025, 1, 1],
  [2000, 2, 29],
  [2023, 6, 15],
  [2100, 2, 28],
  [1999, 7, 7],
];

const hex64 = (n) => "0x" + n.toString(16).padStart(16, "0").toUpperCase();
const dec64 = (n) => n.toString();

function main() {
  const out = {};

  out.stableHash = hashStrings.map((s) => ({
    s,
    hex: hex64(stableHash(s)),
    dec: dec64(stableHash(s)),
    f64: Number(stableHash(s)),
  }));

  out.djb2 = djb2Strings.map((s) => ({ s, v: djb2Hash(s) }));

  out.random101 = randomSeeds.map((seed) => ({ seed, v: dotnetRandomNext101(seed) }));

  out.roundEven = roundValues.map((v) => ({ v, r: roundEven(v) }));

  out.pcl2 = [];
  for (const id of identifiers) {
    for (const [y, m, d] of dates) {
      const date = makeDate(y, m, d);
      const firstSeed = `asdfgbn${dayOfYear(date)}12#3$45${y}IUY`;
      const secondSeed = `QWERTY${id}0*8&6${d}kjhg`;
      const firstHash = Number(stableHash(firstSeed)) / 3;
      const secondHash = Number(stableHash(secondSeed)) / 3;
      out.pcl2.push({
        identifier: id,
        date: `${y}-${pad(m)}-${pad(d)}`,
        score: scoreForDate(date, id),
        dbg: { h1: firstHash, h2: secondHash, raw: Math.abs((firstHash + secondHash) / 527) % 1001 },
      });
    }
  }

  out.pclce = [];
  for (const id of identifiers) {
    for (const [y, m, d] of dates) {
      out.pclce.push({
        identifier: id,
        date: `${y}-${pad(m)}-${pad(d)}`,
        score: pclceScoreForDate(makeDate(y, m, d), id),
      });
    }
  }

  // Write UTF-8 (no BOM) so every consumer reads the same bytes.
  const target = process.argv[2] ?? fileURLToPath(new URL("../tests/vectors/reference.json", import.meta.url));
  writeFileSync(target, JSON.stringify(out, null, 2) + "\n", "utf8");
  console.error("wrote " + target);
}

main();