import { copyFileSync, cpSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const release = join(root, "src-tauri", "target", "release");
const appDir = join(root, "app");
const exeSrc = join(release, "berry-timers.exe");
const exeDst = join(appDir, "Berryworks.exe");

if (!existsSync(exeSrc)) {
  console.error(`Missing ${exeSrc}. Run: npm run tauri -- build --no-bundle`);
  process.exit(1);
}

mkdirSync(join(appDir, "resources"), { recursive: true });
copyFileSync(exeSrc, exeDst);

const dll = join(release, "berry_timers_lib.dll");
if (existsSync(dll)) copyFileSync(dll, join(appDir, "berry_timers_lib.dll"));

const releaseResources = join(release, "resources");
if (existsSync(releaseResources)) {
  cpSync(releaseResources, join(appDir, "resources"), { recursive: true });
}

copyFileSync(join(root, "data", "spells.json"), join(appDir, "resources", "spells.json"));
copyFileSync(join(root, "data", "camps.json"), join(appDir, "resources", "camps.json"));
copyFileSync(join(root, "data", "pets.json"), join(appDir, "resources", "pets.json"));

console.log(`Copied release → ${exeDst}`);
