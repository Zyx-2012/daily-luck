import { readFileSync } from "node:fs";

const js = JSON.parse(readFileSync(new URL("../tests/vectors/reference.json", import.meta.url), "utf8"));
const py = JSON.parse(readFileSync(new URL("../tests/vectors/reference_py.json", import.meta.url), "utf8"));

const mismatches = [];

js.stableHash.forEach((x, i) => {
  const y = py.stableHash[i];
  if (x.hex !== y.hex || x.dec !== y.dec || x.f64 !== y.f64) {
    mismatches.push({
      kind: "stableHash", i, s: x.s,
      js: { hex: x.hex, dec: x.dec, f64: x.f64 },
      py: { hex: y.hex, dec: y.dec, f64: y.f64 },
    });
  }
});

js.djb2.forEach((x, i) => {
  const y = py.djb2[i];
  if (x.v !== y.v) mismatches.push({ kind: "djb2", i, s: x.s, js: x.v, py: y.v });
});

js.random101.forEach((x, i) => {
  const y = py.random101[i];
  if (x.v !== y.v) mismatches.push({ kind: "random101", i, seed: x.seed, js: x.v, py: y.v });
});

js.roundEven.forEach((x, i) => {
  const y = py.roundEven[i];
  if (x.r !== y.r) mismatches.push({ kind: "roundEven", i, v: x.v, js: x.r, py: y.r });
});

js.pcl2.forEach((x, i) => {
  const y = py.pcl2[i];
  if (x.score !== y.score) mismatches.push({ kind: "pcl2", i, id: x.identifier, date: x.date, js: x.score, py: y.score });
});

js.pclce.forEach((x, i) => {
  const y = py.pclce[i];
  if (x.score !== y.score) mismatches.push({ kind: "pclce", i, id: x.identifier, date: x.date, js: x.score, py: y.score });
});

console.log("total vectors:", {
  stableHash: js.stableHash.length,
  djb2: js.djb2.length,
  random101: js.random101.length,
  roundEven: js.roundEven.length,
  pcl2: js.pcl2.length,
  pclce: js.pclce.length,
});

if (mismatches.length === 0) {
  console.log("ALL MATCH ✓");
} else {
  console.log(`MISMATCHES: ${mismatches.length}`);
  for (const m of mismatches) console.log(JSON.stringify(m));
}