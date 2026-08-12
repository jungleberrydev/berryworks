/**
 * Merge spellicon ids from wiki-cache into spells.json and extract/download PNGs.
 *
 * Numeric icons: crop from EQL Spells##.tga atlases (40x40 in 6x6 on 256x256).
 * Letter / special ids: download from eqlwiki File:Spellicon_X.png when available.
 *
 * Usage: node scripts/sync-spell-icons.mjs
 */
import fs from "fs";
import path from "path";
import zlib from "zlib";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const SPELLS_PATH = path.join(ROOT, "data", "spells.json");
const CACHE = path.join(ROOT, "data", "wiki-cache");
const OUT_ICONS = path.join(ROOT, "public", "icons");
const APP_ICONS = path.join(ROOT, "app", "resources", "icons");

const EQ_UIFILES =
  process.env.EQL_UIFILES_DIR ||
  "C:\\Users\\Public\\Daybreak Game Company\\Installed Games\\EverQuest Legends\\uifiles\\default";

const ICON_SIZE = 40;
const ICONS_PER_ROW = 6;
const ICONS_PER_SHEET = 36;
const API = "https://eqlwiki.com/api.php";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function ensureDir(p) {
  fs.mkdirSync(p, { recursive: true });
}

/** Load title → spellicon from wiki-cache. */
function loadIconMap() {
  const map = new Map();
  if (!fs.existsSync(CACHE)) return map;
  for (const f of fs.readdirSync(CACHE)) {
    if (!f.startsWith("spell-") || !f.endsWith(".json")) continue;
    try {
      const j = JSON.parse(fs.readFileSync(path.join(CACHE, f), "utf8"));
      const wt = j.wikitext || "";
      const m = wt.match(/\|\s*spellicon\s*=\s*([^\n|]*)/i);
      if (!m) continue;
      const icon = m[1].trim();
      if (!icon) continue;
      if (j.title) map.set(j.title, icon);
      // Also index by spellname field when present
      const sn = wt.match(/\|\s*spellname\s*=\s*([^\n|]*)/i);
      if (sn?.[1]?.trim()) map.set(sn[1].trim(), icon);
    } catch {
      /* ignore bad cache entries */
    }
  }
  return map;
}

function lookupIcon(iconMap, name) {
  if (iconMap.has(name)) return iconMap.get(name);
  const lower = name.toLowerCase();
  for (const [k, v] of iconMap) {
    if (k.toLowerCase() === lower) return v;
  }
  return "";
}

function crc32(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = c & 1 ? (0xedb88320 ^ (c >>> 1)) : c >>> 1;
  }
  return ~c >>> 0;
}

function pngChunk(type, data) {
  const typeBuf = Buffer.from(type, "ascii");
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crcBuf]);
}

/** Encode RGBA bitmap as PNG (no filter, zlib). */
function encodePngRgba(width, height, rgba) {
  const rowSize = 1 + width * 4;
  const raw = Buffer.alloc(rowSize * height);
  for (let y = 0; y < height; y++) {
    const dest = y * rowSize;
    raw[dest] = 0; // filter None
    rgba.copy(raw, dest + 1, y * width * 4, (y + 1) * width * 4);
  }
  const compressed = zlib.deflateSync(raw, { level: 9 });
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", compressed),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

/** Read uncompressed 32-bit TGA (image type 2). Returns {w,h,rgba top-left}. */
function readTgaRgba(filePath) {
  const buf = fs.readFileSync(filePath);
  const idLen = buf[0];
  const imageType = buf[2];
  if (imageType !== 2 && imageType !== 10) {
    throw new Error(`Unsupported TGA type ${imageType} in ${filePath}`);
  }
  const width = buf.readUInt16LE(12);
  const height = buf.readUInt16LE(14);
  const bpp = buf[16];
  const descriptor = buf[17];
  if (bpp !== 32) throw new Error(`Expected 32bpp TGA, got ${bpp}`);
  const dataOffset = 18 + idLen;
  const topOrigin = (descriptor & 0x20) !== 0;

  let pixels;
  if (imageType === 2) {
    pixels = buf.subarray(dataOffset, dataOffset + width * height * 4);
  } else {
    // RLE (type 10) — decode BGRA
    pixels = Buffer.alloc(width * height * 4);
    let src = dataOffset;
    let dst = 0;
    const total = width * height;
    let count = 0;
    while (count < total) {
      const packet = buf[src++];
      const run = (packet & 0x7f) + 1;
      if (packet & 0x80) {
        const b = buf[src++];
        const g = buf[src++];
        const r = buf[src++];
        const a = buf[src++];
        for (let i = 0; i < run; i++) {
          pixels[dst++] = b;
          pixels[dst++] = g;
          pixels[dst++] = r;
          pixels[dst++] = a;
          count++;
        }
      } else {
        for (let i = 0; i < run; i++) {
          pixels[dst++] = buf[src++];
          pixels[dst++] = buf[src++];
          pixels[dst++] = buf[src++];
          pixels[dst++] = buf[src++];
          count++;
        }
      }
    }
  }

  // Convert BGRA → RGBA, flip if bottom-origin
  const rgba = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) {
    const srcY = topOrigin ? y : height - 1 - y;
    for (let x = 0; x < width; x++) {
      const si = (srcY * width + x) * 4;
      const di = (y * width + x) * 4;
      rgba[di] = pixels[si + 2];
      rgba[di + 1] = pixels[si + 1];
      rgba[di + 2] = pixels[si];
      rgba[di + 3] = pixels[si + 3];
    }
  }
  return { width, height, rgba };
}

function cropIcon(sheetRgba, width, local) {
  const col = local % ICONS_PER_ROW;
  const row = Math.floor(local / ICONS_PER_ROW);
  const out = Buffer.alloc(ICON_SIZE * ICON_SIZE * 4);
  for (let y = 0; y < ICON_SIZE; y++) {
    for (let x = 0; x < ICON_SIZE; x++) {
      const sx = col * ICON_SIZE + x;
      const sy = row * ICON_SIZE + y;
      const si = (sy * width + sx) * 4;
      const di = (y * ICON_SIZE + x) * 4;
      out[di] = sheetRgba[si];
      out[di + 1] = sheetRgba[si + 1];
      out[di + 2] = sheetRgba[si + 2];
      out[di + 3] = sheetRgba[si + 3];
    }
  }
  return out;
}

function safeIconId(id) {
  return String(id).trim().replace(/[^\w.-]+/g, "_");
}

function iconFileName(id) {
  return `spellicon_${safeIconId(id)}.png`;
}

function writeIconBoth(fileName, pngBuf) {
  fs.writeFileSync(path.join(OUT_ICONS, fileName), pngBuf);
  fs.writeFileSync(path.join(APP_ICONS, fileName), pngBuf);
}

async function wikiImageUrl(fileTitle) {
  const url = new URL(API);
  url.searchParams.set("action", "query");
  url.searchParams.set("titles", fileTitle);
  url.searchParams.set("prop", "imageinfo");
  url.searchParams.set("iiprop", "url");
  url.searchParams.set("format", "json");
  const res = await fetch(url, {
    headers: { "User-Agent": "Berryworks/0.1 (spell icons; personal use)" },
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const data = await res.json();
  const pages = data.query?.pages || {};
  for (const page of Object.values(pages)) {
    const u = page.imageinfo?.[0]?.url;
    if (u) return u;
  }
  return null;
}

async function downloadIcon(id) {
  const candidates = [
    `File:Spellicon_${id}.png`,
    `File:Spellicon ${id}.png`,
    `File:spellicon_${id}.png`,
  ];
  for (const title of candidates) {
    try {
      const imgUrl = await wikiImageUrl(title);
      if (!imgUrl) continue;
      const res = await fetch(imgUrl, {
        headers: { "User-Agent": "Berryworks/0.1 (spell icons; personal use)" },
      });
      if (!res.ok) continue;
      const buf = Buffer.from(await res.arrayBuffer());
      writeIconBoth(iconFileName(id), buf);
      await sleep(80);
      return true;
    } catch {
      /* try next */
    }
  }
  return false;
}

function extractNumericFromTga(ids) {
  const sheetsNeeded = new Map(); // sheetNum -> Set of {id, local}
  for (const id of ids) {
    const n = Number(id);
    if (!Number.isFinite(n) || n < 0) continue;
    const sheet = Math.floor(n / ICONS_PER_SHEET) + 1;
    const local = n % ICONS_PER_SHEET;
    if (!sheetsNeeded.has(sheet)) sheetsNeeded.set(sheet, []);
    sheetsNeeded.get(sheet).push({ id: String(n), local });
  }

  let written = 0;
  let missingSheets = 0;
  for (const [sheet, entries] of [...sheetsNeeded.entries()].sort((a, b) => a[0] - b[0])) {
    const tgaPath = path.join(EQ_UIFILES, `Spells${String(sheet).padStart(2, "0")}.tga`);
    if (!fs.existsSync(tgaPath)) {
      console.warn(`  missing sheet ${tgaPath}`);
      missingSheets++;
      continue;
    }
    const { width, rgba } = readTgaRgba(tgaPath);
    for (const { id, local } of entries) {
      const tile = cropIcon(rgba, width, local);
      writeIconBoth(iconFileName(id), encodePngRgba(ICON_SIZE, ICON_SIZE, tile));
      written++;
    }
  }
  return { written, missingSheets };
}

async function main() {
  ensureDir(OUT_ICONS);
  ensureDir(APP_ICONS);

  const iconMap = loadIconMap();
  console.log(`Wiki-cache icons: ${iconMap.size} title mappings`);

  const spells = JSON.parse(fs.readFileSync(SPELLS_PATH, "utf8"));
  const needed = new Set();
  let withIcon = 0;
  for (const spell of spells) {
    const icon = lookupIcon(iconMap, spell.name);
    if (icon) {
      spell.spellicon = icon;
      needed.add(icon);
      withIcon++;
    } else if (spell.spellicon) {
      needed.add(spell.spellicon);
      withIcon++;
    } else {
      delete spell.spellicon;
    }
  }
  fs.writeFileSync(SPELLS_PATH, JSON.stringify(spells, null, 2));
  // Keep app resources spells.json in sync when present
  const appSpells = path.join(ROOT, "app", "resources", "spells.json");
  if (fs.existsSync(path.dirname(appSpells))) {
    ensureDir(path.dirname(appSpells));
    fs.copyFileSync(SPELLS_PATH, appSpells);
  }

  console.log(`Spells with spellicon: ${withIcon}/${spells.length}; unique ids: ${needed.size}`);

  const numeric = [...needed].filter((id) => /^\d+$/.test(id));
  const other = [...needed].filter((id) => !/^\d+$/.test(id));

  console.log(`Extracting ${numeric.length} numeric icons from TGA…`);
  const { written, missingSheets } = extractNumericFromTga(numeric);
  console.log(`  wrote ${written} from TGA (missing sheets: ${missingSheets})`);

  // Any numeric still missing → wiki fallback
  let downloaded = 0;
  let failed = 0;
  for (const id of numeric) {
    const dest = path.join(OUT_ICONS, iconFileName(id));
    if (fs.existsSync(dest)) continue;
    process.stdout.write(`  wiki fallback ${id}… `);
    const ok = await downloadIcon(id);
    console.log(ok ? "ok" : "fail");
    if (ok) downloaded++;
    else failed++;
  }

  console.log(`Downloading ${other.length} non-numeric icons from wiki…`);
  for (const id of other) {
    process.stdout.write(`  ${id}… `);
    const ok = await downloadIcon(id);
    console.log(ok ? "ok" : "fail");
    if (ok) downloaded++;
    else failed++;
  }

  const present = fs.readdirSync(OUT_ICONS).filter((f) => f.endsWith(".png")).length;
  console.log(`Done. Icons on disk: ${present}; downloaded: ${downloaded}; failed: ${failed}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
