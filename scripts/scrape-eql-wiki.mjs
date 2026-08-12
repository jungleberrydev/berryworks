/**
 * Scrape EQL wiki class pages + Spellpage templates into data/spells.json
 * Usage: node scripts/scrape-eql-wiki.mjs
 */
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const OUT = path.join(ROOT, "data", "spells.json");
const CACHE = path.join(ROOT, "data", "wiki-cache");
const API = "https://eqlwiki.com/api.php";

const CLASSES = [
  "Warrior",
  "Cleric",
  "Paladin",
  "Ranger",
  "Shadow_Knight",
  "Druid",
  "Monk",
  "Bard",
  "Rogue",
  "Shaman",
  "Necromancer",
  "Wizard",
  "Magician",
  "Enchanter",
  "Beastlord",
  "Berserker",
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function api(params) {
  const url = new URL(API);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  url.searchParams.set("format", "json");
  const res = await fetch(url, {
    headers: { "User-Agent": "Berryworks/0.1 (spell import; personal use)" },
  });
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);
  return res.json();
}

function ensureDir(p) {
  fs.mkdirSync(p, { recursive: true });
}

function cachePath(key) {
  const safe = key.replace(/[^\w.-]+/g, "_");
  return path.join(CACHE, `${safe}.json`);
}

async function cached(key, fn) {
  ensureDir(CACHE);
  const p = cachePath(key);
  if (fs.existsSync(p)) return JSON.parse(fs.readFileSync(p, "utf8"));
  const data = await fn();
  fs.writeFileSync(p, JSON.stringify(data));
  await sleep(150);
  return data;
}

/** Collect spell titles listed on a class page (from parse links + tooltip text). */
async function spellsFromClassPage(page) {
  const data = await cached(`class-${page}`, () =>
    api({ action: "parse", page, prop: "text|links", redirect: "1" })
  );
  if (data.error) {
    console.warn(`  skip ${page}: ${data.error.info || data.error.code}`);
    return [];
  }
  const titles = new Set();
  for (const link of data.parse?.links || []) {
    if (link.ns === 0 && link.exists !== undefined) {
      // MediaWiki marks missing with "exists" absent; present links have exists:""
      titles.add(link["*"]);
    }
  }
  // Also pull names from "SpellName CLASS(level)" patterns in HTML text
  const html = data.parse?.text?.["*"] || "";
  const re = /([A-Za-z][A-Za-z0-9'`:.\- ]{1,60}?)\s+(?:WAR|CLR|PAL|RNG|SHD|DRU|MNK|BRD|ROG|SHM|NEC|WIZ|MAG|ENC|BST|BER)\(\d+\)/g;
  let m;
  while ((m = re.exec(html))) {
    titles.add(m[1].trim());
  }
  return [...titles];
}

async function allCategorySpells() {
  const titles = [];
  let cont = null;
  let page = 0;
  do {
    const params = {
      action: "query",
      list: "categorymembers",
      cmtitle: "Category:Spells",
      cmlimit: "500",
    };
    if (cont) params.cmcontinue = cont;
    const data = await cached(`cat-spells-${page}`, () => api(params));
    for (const m of data.query?.categorymembers || []) {
      if (m.ns === 0) titles.push(m.title);
    }
    cont = data.continue?.cmcontinue || null;
    page += 1;
    console.log(`  Category:Spells page ${page}: ${titles.length} so far`);
  } while (cont);
  return titles;
}

function chunk(arr, n) {
  const out = [];
  for (let i = 0; i < arr.length; i += n) out.push(arr.slice(i, i + n));
  return out;
}

async function fetchWikitextBatch(titles) {
  const key = `wt-${titles.map((t) => t.slice(0, 20)).join("_")}`.slice(0, 80);
  // Don't use one giant cache key for batches — fetch live with light cache per title
  const missing = [];
  const result = new Map();
  for (const t of titles) {
    const p = cachePath(`spell-${t}`);
    if (fs.existsSync(p)) {
      result.set(t, JSON.parse(fs.readFileSync(p, "utf8")));
    } else {
      missing.push(t);
    }
  }
  for (const batch of chunk(missing, 40)) {
    const data = await api({
      action: "query",
      prop: "revisions",
      rvprop: "content",
      rvslots: "main",
      titles: batch.join("|"),
      redirects: "1",
    });
    await sleep(200);
    const pages = data.query?.pages || {};
    const redirects = {};
    for (const r of data.query?.redirects || []) redirects[r.from] = r.to;
    for (const page of Object.values(pages)) {
      const title = page.title;
      const wikitext = page.revisions?.[0]?.slots?.main?.["*"] ?? page.revisions?.[0]?.["*"] ?? "";
      const payload = { title, wikitext, missing: !!page.missing };
      result.set(title, payload);
      fs.writeFileSync(cachePath(`spell-${title}`), JSON.stringify(payload));
    }
    console.log(`  fetched wikitext batch (${batch.length}), cache size growing`);
  }
  return result;
}

function parseTemplateField(wikitext, field) {
  const re = new RegExp(`\\|\\s*${field}\\s*=\\s*([^\\n|]*)`, "i");
  const m = wikitext.match(re);
  return m ? m[1].trim() : "";
}

function durationToTicks(duration) {
  if (!duration) return null;
  let d = duration.toLowerCase().trim();
  if (!d || d === "instant" || d === "permanent" || d.includes("permanent")) return null;
  // Scaled wiki strings: use the max (last) duration.
  if (d.includes(" to ")) d = d.split(" to ").pop().trim();
  // Drop "@L57" and restatements like "(1 hour)" so minutes aren't double-counted.
  d = d.replace(/@l\d+/gi, " ").replace(/\([^)]*\)/g, " ").replace(/\s+/g, " ").trim();

  let ticks = null;
  const tickMatch = d.match(/(\d+)\s*ticks?/);
  if (tickMatch) ticks = Number(tickMatch[1]);

  let seconds = 0;
  const hourMatch = d.match(/(\d+)\s*hours?\b/);
  if (hourMatch) seconds += Number(hourMatch[1]) * 3600;
  const minMatch = d.match(/(\d+)\s*min(?:ute)?s?/);
  if (minMatch) seconds += Number(minMatch[1]) * 60;
  // Prefer "24 sec" / "24 seconds" before bare "24s" so "secs" isn't misread.
  const secMatch = d.match(/(\d+)\s*sec(?:ond)?s?\b/);
  if (secMatch) {
    seconds += Number(secMatch[1]);
  } else {
    // Wiki often uses "24s" / "60s" (no "sec" word).
    const bareSec = d.match(/(\d+)\s*s\b/);
    if (bareSec) seconds += Number(bareSec[1]);
  }
  // "16 Min" / bare "27m" — only when nothing else parsed as time
  if (!tickMatch && seconds === 0) {
    const bareMin = d.match(/^(\d+)\s*m(?:in)?$/);
    if (bareMin) seconds = Number(bareMin[1]) * 60;
  }

  if (ticks == null && seconds > 0) ticks = Math.round(seconds / 6);
  if (ticks == null || ticks <= 0) return null;
  return ticks;
}

function landOtherPattern(msg) {
  if (!msg) return "";
  let s = msg.trim().replace(/\.$/, "");
  // "Someone has been mesmerized" → "has been mesmerized"
  s = s.replace(/^Someone\s+/i, "");
  // Wiki stubs like "Someone ." collapse to empty / "."
  if (!s || s === ".") return "";
  // "so-and-so ..." variants aren't used; keep remainder
  return s;
}

function cleanLandYou(msg) {
  if (!msg) return "";
  const s = msg.trim().replace(/\.$/, "");
  if (!s || /^you\s*$/i.test(s)) return "";
  return s;
}

function categorize(spellType, durationField, name) {
  const t = (spellType || "").toLowerCase();
  const n = name.toLowerCase();
  if (t.includes("damage over time") || /\bdot\b/.test(t)) return "dot";
  if (t.includes("heal over time") || /\bhot\b/.test(t) || t === "heal") return "buff";
  if (t.includes("beneficial") || t.includes("buff") || t.includes("statistic")) return "buff";
  if (t.includes("detrimental") || t.includes("debuff")) return "debuff";
  // Heuristics
  if (/mesmerize|enthrall|entrance|dazzle|root|snare|fear|lull|soothe|pacify|tash|weaken|slow|drowsy/.test(n))
    return "debuff";
  if (
    /shield|clarity|strengthen|haste|skin|coat|aura|blessing|regeneration|chloroplast|healing/.test(n)
  )
    return "buff";
  return t.includes("utility") ? "buff" : "debuff";
}

function tierPct(category) {
  return category === "dot" ? 5 : 10;
}

function parseSpell(title, wikitext) {
  if (!wikitext || !/\{\{\s*Spellpage/i.test(wikitext)) return null;

  const durationRaw = parseTemplateField(wikitext, "duration");
  const ticks = durationToTicks(durationRaw);
  if (ticks == null) return null; // skip instant/permanent/unknown

  const spellType = parseTemplateField(wikitext, "spell_type");
  const landYou = cleanLandYou(parseTemplateField(wikitext, "msg_cast_on_you"));
  const landOtherRaw = parseTemplateField(wikitext, "msg_cast_on_other");
  const wearOff = cleanLandYou(parseTemplateField(wikitext, "msg_wears_off"));
  const name = parseTemplateField(wikitext, "spellname") || title;
  const category = categorize(spellType, durationRaw, name);
  const spellicon = parseTemplateField(wikitext, "spellicon");

  // Prefer fixed ticks from wiki; mark formula fixed for v1
  return {
    name,
    category,
    duration_formula: "fixed",
    base_ticks: ticks,
    max_ticks: ticks,
    tier_duration_pct: tierPct(category),
    land_other: landOtherPattern(landOtherRaw),
    land_you: landYou,
    wear_off_you: wearOff,
    watched_by_default: false,
    classes: parseClasses(wikitext),
    spellicon,
    wiki_duration: durationRaw,
    spell_type: spellType,
  };
}

function normalizeClassName(name) {
  const map = {
    Shadowknight: "Shadow Knight",
    ShadowKnight: "Shadow Knight",
    Shadow_Knight: "Shadow Knight",
  };
  return map[name] || name;
}

function parseClasses(wikitext) {
  const block = wikitext.match(/\|\s*classes\s*=([\s\S]*?)(?=\n\|\s*\w+\s*=)/i);
  if (!block) return [];
  const found = [];
  const seen = new Set();
  const re = /\[\[([^\]]+)\]\][^\n]*Level\s+(\d+)/gi;
  let m;
  while ((m = re.exec(block[1]))) {
    const cls = normalizeClassName(m[1]);
    const level = Number(m[2]);
    const key = `${cls}:${level}`;
    if (seen.has(key)) continue;
    seen.add(key);
    found.push({ class: cls, level });
  }
  return found;
}

function watchedDefaults(name, category) {
  const n = name.toLowerCase();
  if (
    /^(mesmerize|enthrall|entrance|dazzle|root|ensnaring roots|snare|fear|clarity|tashan|tashani|tashania|tashanian)$/i.test(
      n
    )
  )
    return true;
  if (category === "debuff" && /mez|root|snare|lull|soothe|pacify|charm/.test(n)) return true;
  return false;
}

async function main() {
  console.log("Collecting spells from class pages…");
  const fromClasses = new Set();
  const classHits = {};
  for (const cls of CLASSES) {
    process.stdout.write(`  ${cls}… `);
    const titles = await spellsFromClassPage(cls);
    classHits[cls] = titles.length;
    titles.forEach((t) => fromClasses.add(t));
    console.log(`${titles.length} links`);
  }

  console.log("Collecting Category:Spells…");
  const fromCat = await allCategorySpells();
  fromCat.forEach((t) => fromClasses.add(t));

  // Filter obvious non-spells / NPC names / AA noise lightly later via Spellpage check
  const allTitles = [...fromClasses].sort((a, b) => a.localeCompare(b));
  console.log(`Unique titles to inspect: ${allTitles.length}`);

  console.log("Fetching Spellpage wikitext…");
  const spells = [];
  const seen = new Set();
  for (const batch of chunk(allTitles, 40)) {
    const map = await fetchWikitextBatch(batch);
    for (const [title, payload] of map) {
      if (payload.missing) continue;
      const parsed = parseSpell(title, payload.wikitext);
      if (!parsed) continue;
      const key = parsed.name.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      parsed.watched_by_default = watchedDefaults(parsed.name, parsed.category);
      spells.push(parsed);
    }
  }

  spells.sort((a, b) => a.name.localeCompare(b.name));

  // Keep a rich sidecar; ship runtime fields including classes for UI grouping
  const richPath = path.join(ROOT, "data", "spells.wiki.json");
  fs.writeFileSync(richPath, JSON.stringify(spells, null, 2));

  // Prefer any local corrections already in data/spells.json (e.g. Jungleberry Clarity msgs)
  const existingPath = OUT;
  const existingByName = new Map();
  if (fs.existsSync(existingPath)) {
    try {
      for (const s of JSON.parse(fs.readFileSync(existingPath, "utf8"))) {
        existingByName.set(s.name, s);
      }
    } catch {
      /* ignore */
    }
  }

  const shipped = spells.map((spell) => {
    const prev = existingByName.get(spell.name);
    const row = {
      name: spell.name,
      category: prev?.category ?? spell.category,
      duration_formula: prev?.duration_formula ?? spell.duration_formula,
      base_ticks: prev?.base_ticks ?? spell.base_ticks,
      max_ticks: prev?.max_ticks ?? spell.max_ticks,
      tier_duration_pct: prev?.tier_duration_pct ?? spell.tier_duration_pct,
      land_other: prev?.land_other ?? spell.land_other,
      land_you: prev?.land_you ?? spell.land_you,
      wear_off_you: prev?.wear_off_you ?? spell.wear_off_you,
      watched_by_default: prev?.watched_by_default ?? spell.watched_by_default,
      classes: spell.classes || [],
    };
    const icon = spell.spellicon || prev?.spellicon;
    if (icon) row.spellicon = icon;
    return row;
  });

  fs.writeFileSync(OUT, JSON.stringify(shipped, null, 2));

  const summary = {
    classHits,
    uniqueTitles: allTitles.length,
    timedSpells: shipped.length,
    watchedDefaults: shipped.filter((s) => s.watched_by_default).length,
    byCategory: shipped.reduce((acc, s) => {
      acc[s.category] = (acc[s.category] || 0) + 1;
      return acc;
    }, {}),
  };
  fs.writeFileSync(path.join(ROOT, "data", "scrape-summary.json"), JSON.stringify(summary, null, 2));
  console.log("Done:", summary);
  console.log(`Wrote ${OUT} (${shipped.length} timed spells)`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
