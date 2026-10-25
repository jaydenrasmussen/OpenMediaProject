// Every visible word goes through t() so a translation is one more table.
// English is the source; a language that lacks a key falls back to it.

const en = {
  "nav.movies": "Movies",
  "nav.discover": "Discover",
  "nav.series": "Series",
  "nav.collections": "Collections",
  "nav.addMovie": "Add movie",
  "nav.addSeries": "Add series",
  "nav.import": "Import",
  "nav.calendar": "Calendar",
  "nav.wanted": "Wanted",
  "nav.activity": "Activity",
  "nav.settings": "Settings",
  "nav.system": "System",
  "nav.signOut": "Sign out",
  "common.loading": "Loading…",
  "common.save": "Save",
  "common.cancel": "Cancel",
  "settings.title": "Settings",
  "settings.media": "Media management",
  "settings.profiles": "Quality profiles",
  "settings.formats": "Custom formats",
  "settings.sizes": "Quality sizes",
  "settings.rules": "Delays & tags",
  "settings.indexers": "Indexers",
  "settings.clients": "Download clients",
  "settings.lists": "Lists",
  "settings.notifications": "Notifications",
  "settings.mediaServers": "Media servers",
  "settings.plugins": "Plugins",
  "settings.vpn": "VPN",
  "settings.ui": "UI",
  "settings.general": "General",
} as const;

export type Key = keyof typeof en;

/** Translations by language code. Add a language by adding a table here. */
const tables: Record<string, Partial<Record<Key, string>>> = { en };

export const LANGUAGES: { code: string; name: string }[] = [{ code: "en", name: "English" }];

let current = "en";

export function setLanguage(code: string) {
  current = code in tables ? code : "en";
}

export function t(key: Key): string {
  return tables[current]?.[key] ?? en[key];
}
