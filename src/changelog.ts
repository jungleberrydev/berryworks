import changelogMarkdown from "../CHANGELOG.md?raw";

export interface ChangelogSection {
  title: string;
  items: string[];
}

export interface ChangelogEntry {
  version: string;
  date: string | null;
  summary: string | null;
  sections: ChangelogSection[];
}

const LAST_SEEN_VERSION_KEY = "berryworks-last-seen-version";

export function parseChangelog(md: string): ChangelogEntry[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const entries: ChangelogEntry[] = [];
  let current: ChangelogEntry | null = null;
  let section: ChangelogSection | null = null;
  const summaryParts: string[] = [];

  const flushSummary = () => {
    if (!current || current.summary || summaryParts.length === 0) return;
    current.summary = summaryParts.join(" ").trim() || null;
    summaryParts.length = 0;
  };

  for (const raw of lines) {
    const line = raw.trimEnd();
    const heading = line.match(/^## \[([^\]]+)\](?:\s*-\s*(.+))?$/);
    if (heading) {
      flushSummary();
      if (current) entries.push(current);
      current = {
        version: heading[1],
        date: heading[2]?.trim() || null,
        summary: null,
        sections: [],
      };
      section = null;
      summaryParts.length = 0;
      continue;
    }
    if (!current) continue;
    if (/^\[[^\]]+\]:\s+\S/.test(line)) continue;

    const sub = line.match(/^###\s+(.+)/);
    if (sub) {
      flushSummary();
      section = { title: sub[1].trim(), items: [] };
      current.sections.push(section);
      continue;
    }

    const item = line.match(/^[-*]\s+(.+)/);
    if (item && section) {
      section.items.push(item[1].trim());
      continue;
    }

    const text = line.trim();
    if (text && !section && !text.startsWith("#")) {
      summaryParts.push(text);
    }
  }
  flushSummary();
  if (current) entries.push(current);
  return entries;
}

export function entryHasContent(entry: ChangelogEntry): boolean {
  if (entry.summary) return true;
  return entry.sections.some((s) => s.items.length > 0);
}

function parseSemver(version: string): [number, number, number] | null {
  const m = version.trim().match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3])];
}

/** Negative if a < b, 0 if equal, positive if a > b. Unreleased sorts newest. */
export function compareVersions(a: string, b: string): number {
  const pa = parseSemver(a);
  const pb = parseSemver(b);
  if (!pa && !pb) return 0;
  if (!pa) return 1;
  if (!pb) return -1;
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

export function displayVersionLabel(entry: ChangelogEntry): string {
  if (entry.version.toLowerCase() === "unreleased") return "This build";
  return entry.version;
}

/** Notes to show after an update: versions newer than lastSeen, up through current. */
export function entriesSince(
  entries: ChangelogEntry[],
  lastSeen: string | null,
  current: string
): ChangelogEntry[] {
  return entries.filter((entry) => {
    if (!entryHasContent(entry)) return false;
    if (entry.version.toLowerCase() === "unreleased") {
      if (!lastSeen) return false;
      return compareVersions(current, lastSeen) > 0;
    }
    if (current && compareVersions(entry.version, current) > 0) return false;
    if (!lastSeen) return compareVersions(entry.version, current) === 0;
    return compareVersions(entry.version, lastSeen) > 0;
  });
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function formatInline(s: string): string {
  let t = escapeHtml(s);
  t = t.replace(/`([^`]+)`/g, "<code>$1</code>");
  t = t.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  t = t.replace(
    /\[([^\]]+)\]\((https?:[^)]+)\)/g,
    '<a href="$2" target="_blank" rel="noopener">$1</a>'
  );
  return t;
}

export function renderChangelogHtml(entries: ChangelogEntry[]): string {
  const visible = entries.filter(entryHasContent);
  if (!visible.length) {
    return `<p class="hint">No release notes yet.</p>`;
  }
  return visible
    .map((entry) => {
      const label = escapeHtml(displayVersionLabel(entry));
      const date = entry.date
        ? ` <span class="changelog-date">${escapeHtml(entry.date)}</span>`
        : "";
      const summary = entry.summary
        ? `<p class="changelog-summary">${formatInline(entry.summary)}</p>`
        : "";
      const sections = entry.sections
        .filter((sec) => sec.items.length)
        .map((sec) => {
          const items = sec.items.map((item) => `<li>${formatInline(item)}</li>`).join("");
          return `<h4>${escapeHtml(sec.title)}</h4><ul>${items}</ul>`;
        })
        .join("");
      return `<article class="changelog-entry">
      <h3>${label}${date}</h3>
      ${summary}
      ${sections}
    </article>`;
    })
    .join("");
}

export function loadChangelog(): ChangelogEntry[] {
  return parseChangelog(changelogMarkdown);
}

export function getLastSeenVersion(): string | null {
  try {
    return localStorage.getItem(LAST_SEEN_VERSION_KEY);
  } catch {
    return null;
  }
}

export function setLastSeenVersion(version: string): void {
  try {
    localStorage.setItem(LAST_SEEN_VERSION_KEY, version);
  } catch {
    /* ignore quota / private mode */
  }
}
