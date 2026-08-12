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
