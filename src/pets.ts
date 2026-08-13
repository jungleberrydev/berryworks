/**
 * EQL pet target detection (mirrors `data/pets.json` + Rust `pets.rs`).
 *
 * Log evidence:
 * - Other pets: `{OwnerName} pet` (Hoptor Thaggelum pet, Miragul pet, a shadowknight pet)
 * - Your pet wear-off: `Your pet's {Spell} spell has worn off.`
 * - Your pet land target: unique display name (e.g. Gastik) — set my_pet_name
 */

/** Documented generic type / model names; keep in sync with data/pets.json type_names. */
export const PET_TYPE_NAMES: readonly string[] = [
  "an earth elemental",
  "a fire elemental",
  "an air elemental",
  "a water elemental",
  "Earth Elemental",
  "Fire Elemental",
  "Air Elemental",
  "Water Elemental",
  "a skeleton",
  "a decaying skeleton",
  "a bone construct",
  "an undead knight",
  "a warder",
  "Spirit of Sharik",
  "Spirit of Khaliz",
  "Spirit of Keshuval",
  "Spirit of Herikol",
  "Spirit of Yekan",
  "Spirit of Kashek",
  "an animation",
  "a swirling animation",
  "a spirit guardian",
  "a guardian spirit",
  "Companion Spirit",
  "Vigilant Spirit",
  "Guardian Spirit",
  "Frenzied Spirit",
  "Spirit of the Howler",
];

export function isMyPet(target: string, myPetName: string | undefined | null): boolean {
  const mine = (myPetName ?? "").trim();
  if (!mine) return false;
  return target.trim().toLowerCase() === mine.toLowerCase();
}

export function isPetTarget(
  target: string,
  typeNames: readonly string[] = PET_TYPE_NAMES
): boolean {
  const t = target.trim();
  if (!t || t.toLowerCase() === "you") return false;
  if (t.toLowerCase().endsWith(" pet")) return true;
  const lower = t.toLowerCase();
  return typeNames.some((n) => n.trim().toLowerCase() === lower);
}

/** Class / rank titles for generic NPCs (`Cleric of Innoruuk`). Keep in sync with engine.rs. */
const GENERIC_NPC_TITLES = new Set([
  "acolyte",
  "apostle",
  "avenger",
  "bard",
  "beastlord",
  "champion",
  "cleric",
  "defender",
  "devotee",
  "disciple",
  "druid",
  "enchanter",
  "fanatic",
  "guardian",
  "hand",
  "herald",
  "high cleric",
  "high priest",
  "initiate",
  "knight",
  "lady",
  "lord",
  "magician",
  "monk",
  "necromancer",
  "oracle",
  "paladin",
  "priest",
  "prophet",
  "ranger",
  "rogue",
  "sentinel",
  "servant",
  "shadow knight",
  "shadowknight",
  "shaman",
  "templar",
  "warrior",
  "wizard",
  "zealot",
]);

/** Creature-type tokens so `spite golem` matches after the article is stripped. */
const GENERIC_NPC_TYPES = new Set([
  "abhorrent",
  "banshee",
  "basilisk",
  "beetle",
  "boar",
  "bouncer",
  "chest",
  "construct",
  "cub",
  "drake",
  "elemental",
  "fiend",
  "gargoyle",
  "ghast",
  "ghost",
  "ghoul",
  "giant",
  "gnoll",
  "goblin",
  "golem",
  "griffin",
  "guard",
  "horror",
  "imp",
  "kobold",
  "lich",
  "minion",
  "mummy",
  "ogre",
  "orc",
  "pawn",
  "rat",
  "revenant",
  "scarecrow",
  "skeleton",
  "snake",
  "spider",
  "spirit",
  "treant",
  "vampire",
  "wolf",
  "wraith",
  "wyvern",
  "zombie",
]);

function stripLeadingArticle(t: string): string {
  if (t.startsWith("an ")) return t.slice(3).trimStart();
  if (t.startsWith("the ")) return t.slice(4).trimStart();
  if (t.startsWith("a ")) return t.slice(2).trimStart();
  return t;
}

/**
 * Generic NPC names whose beneficial self-buffs share land text with player
 * haste/SoW. Mirrors Rust `looks_like_unnamed_npc`.
 */
export function looksLikeNpcBuffTarget(target: string): boolean {
  const t = target.trim().toLowerCase();
  if (!t || t === "you") return false;
  const rest = stripLeadingArticle(t);
  if (t.startsWith("a ") || t.startsWith("an ") || t.startsWith("the ")) return true;
  const ofIdx = rest.indexOf(" of ");
  if (ofIdx > 0) {
    const before = rest.slice(0, ofIdx);
    if (GENERIC_NPC_TITLES.has(before) || [...GENERIC_NPC_TITLES].some((title) => before.endsWith(` ${title}`))) {
      return true;
    }
  }
  return rest.split(/[^a-z]+/).some((w) => w.length > 0 && GENERIC_NPC_TYPES.has(w));
}

/** Main/friendly overlay: self-buffs-only and/or hide-other-pets. */
export function keepFriendlyTarget(
  target: string,
  opts: {
    selfBuffsOnly: boolean;
    hideOtherPets: boolean;
    myPetName: string;
  }
): boolean {
  const mine = isMyPet(target, opts.myPetName);
  if (opts.selfBuffsOnly) {
    if (target.toLowerCase() !== "you" && !mine) return false;
  }
  if (opts.hideOtherPets) {
    const pet = mine || isPetTarget(target);
    if (pet && !mine) return false;
  }
  return true;
}

/** Short hint for settings UI. */
export const PET_NAME_HINT =
  "Exact log target for your pet (e.g. Gastik, or Jungleberry pet). Other pets usually appear as OwnerName pet.";
