/**
 * Bulk-import buff duration formulas from EQL spells_us.txt into data/spells.json.
 * Field [11] = formula ID, [12] = duration cap (ticks). Lowest spell ID wins per name.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const SPELLS_PATH = path.join(ROOT, "data", "spells.json");
const RESOURCE_PATH = path.join(ROOT, "app", "resources", "spells.json");
const SUMMARY_PATH = path.join(ROOT, "data", "duration-import-summary.json");
const CLIENT_PATH =
  "C:\\Users\\Public\\Daybreak Game Company\\Installed Games\\EverQuest Legends\\spells_us.txt";

const FORMULA_MAP = {
  1: "level_div_2",
  2: "level_div_2_plus_5",
  3: "level_x30",
  4: "fixed_50",
  5: "fixed_2",
  6: "level_div_2_plus_2",
  7: "level",
  8: "level_plus_10",
  9: "level_x2_plus_10",
  10: "level_x3_plus_10",
  11: "level_plus_3_x30",
  12: "level_div_4",
  13: "level_x4_plus_10",
  14: "level_plus_2_x5",
  15: "level_plus_10_x10",
  50: "f50",
  51: "permanent",
};

const LEVEL_FORMULAS = new Set([1, 2, 3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

function calcTicks(formula, level, cap) {
  let ticks;
  switch (formula) {
    case 1:
      ticks = Math.floor(level / 2);
      break;
    case 2:
      ticks = level > 3 ? Math.floor(level / 2) + 5 : 6;
      break;
    case 3:
      ticks = 30 * level;
      break;
    case 4:
      ticks = 50;
      break;
    case 5:
      ticks = 2;
      break;
    case 6:
      ticks = Math.floor(level / 2) + 2;
      break;
    case 7:
      ticks = level;
      break;
    case 8:
      ticks = level + 10;
      break;
    case 9:
      ticks = 2 * level + 10;
      break;
    case 10:
      ticks = 3 * level + 10;
      break;
    case 11:
      ticks = 30 * (level + 3);
      break;
    case 12:
      ticks = level > 7 ? Math.floor(level / 4) : 1;
      break;
    case 13:
      ticks = 4 * level + 10;
      break;
    case 14:
      ticks = 5 * (level + 2);
      break;
    case 15:
      ticks = 10 * (level + 10);
      break;
    default:
      return null;
  }
  if (cap > 0 && ticks > cap) ticks = cap;
  if (ticks < 1) ticks = 1;
  return ticks;
}

function minCastLevel(spell) {
  const levels = (spell.classes || [])
    .map((c) => Number(c.level) || 0)
    .filter((l) => l > 0);
  return levels.length ? Math.min(...levels) : 1;
}

console.log("Indexing client spells (lowest ID wins)...");
const byName = new Map();
const raw = fs.readFileSync(CLIENT_PATH, "utf8");
for (const line of raw.split(/\r?\n/)) {
  if (!line) continue;
  const f = line.split("^");
  if (f.length < 13) continue;
  const id = Number.parseInt(f[0], 10);
  if (!Number.isFinite(id)) continue;
  const name = (f[1] || "").trim();
  if (!name) continue;
  const formula = Number.parseInt(f[11], 10) || 0;
  const cap = Number.parseInt(f[12], 10) || 0;
  const key = name.toLowerCase();
  const prev = byName.get(key);
  if (!prev || id < prev.id) {
    byName.set(key, { id, name, formula, cap });
  }
}
console.log(`Client unique names: ${byName.size}`);

const spells = JSON.parse(fs.readFileSync(SPELLS_PATH, "utf8"));
const fixed = [];
const skipped = [];
const unmatched = [];
const skipReasons = {};

function addSkip(reason, name) {
  skipReasons[reason] = (skipReasons[reason] || 0) + 1;
  skipped.push({ name, reason });
}

for (const spell of spells) {
  const key = String(spell.name).toLowerCase();
  const c = byName.get(key);
  if (!c) {
    unmatched.push(spell.name);
    continue;
  }

  const fid = c.formula;
  const cap = c.cap;

  if (
    fid === 50 ||
    fid === 51 ||
    spell.duration_formula === "f50" ||
    spell.duration_formula === "permanent"
  ) {
    addSkip("permanent_f50", spell.name);
    continue;
  }

  if (fid === 0 && cap === 0) {
    addSkip("not_a_buff", spell.name);
    continue;
  }

  const isTrueFixed =
    fid === 4 || fid === 5 || fid >= 200 || (fid === 0 && cap > 0);
  if (isTrueFixed) {
    addSkip("true_fixed", spell.name);
    continue;
  }

  if (!LEVEL_FORMULAS.has(fid)) {
    addSkip(`unknown_formula_${fid}`, spell.name);
    continue;
  }

  const minLvl = minCastLevel(spell);
  const calcAtMin = calcTicks(fid, minLvl, cap);
  if (calcAtMin == null) {
    addSkip("calc_failed", spell.name);
    continue;
  }

  if (cap > 0 && calcAtMin >= cap) {
    addSkip("at_cap_by_cast_level", spell.name);
    continue;
  }

  const formulaStr = FORMULA_MAP[fid];
  const prev = `${spell.duration_formula}/${spell.base_ticks}/${spell.max_ticks}`;
  const changed =
    spell.duration_formula !== formulaStr ||
    spell.max_ticks !== cap ||
    spell.base_ticks !== calcAtMin;

  spell.duration_formula = formulaStr;
  spell.max_ticks = cap;
  spell.base_ticks = calcAtMin;

  if (changed) {
    fixed.push({
      name: spell.name,
      clientId: c.id,
      formulaId: fid,
      formula: formulaStr,
      max_ticks: cap,
      base_ticks: calcAtMin,
      minLevel: minLvl,
      prev,
    });
  } else {
    addSkip("already_correct", spell.name);
  }
}

fs.writeFileSync(SPELLS_PATH, JSON.stringify(spells, null, 2) + "\n", "utf8");
fs.mkdirSync(path.dirname(RESOURCE_PATH), { recursive: true });
fs.copyFileSync(SPELLS_PATH, RESOURCE_PATH);

const formulaCounts = {};
for (const s of spells) {
  formulaCounts[s.duration_formula] = (formulaCounts[s.duration_formula] || 0) + 1;
}

const summary = {
  generated_at: new Date().toISOString(),
  total_spells: spells.length,
  client_names_indexed: byName.size,
  fixed: fixed.length,
  skipped: skipped.length,
  unmatched: unmatched.length,
  skip_reasons: skipReasons,
  formula_counts_after: Object.fromEntries(
    Object.entries(formulaCounts).sort((a, b) => b[1] - a[1]),
  ),
  fixed_by_formula: fixed.reduce((acc, f) => {
    acc[f.formula] = (acc[f.formula] || 0) + 1;
    return acc;
  }, {}),
  fixed_sample: fixed.slice(0, 50),
  unmatched_names: unmatched.slice().sort(),
};

fs.writeFileSync(SUMMARY_PATH, JSON.stringify(summary, null, 2) + "\n", "utf8");

console.log("");
console.log(`FIXED: ${fixed.length}`);
console.log(`SKIPPED: ${skipped.length}`);
console.log(`UNMATCHED: ${unmatched.length}`);
console.log("Skip reasons:");
for (const [k, v] of Object.entries(skipReasons).sort((a, b) => b[1] - a[1])) {
  console.log(`  ${String(v).padStart(5)}  ${k}`);
}
console.log("Formula counts after:");
for (const [k, v] of Object.entries(formulaCounts).sort((a, b) => b[1] - a[1])) {
  console.log(`  ${String(v).padStart(5)}  ${k}`);
}
console.log("Sample fixes:");
for (const f of fixed.slice(0, 25)) {
  console.log(
    `  ${f.name} -> ${f.formula} max=${f.max_ticks} base=${f.base_ticks} (was ${f.prev})`,
  );
}
