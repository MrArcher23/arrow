// App-chrome appearance (light/dark), isolated the same way as zoom.ts / window.ts /
// platform.ts. This is DISTINCT from themes.ts: that one picks the CodeMirror syntax
// theme for the diff editor, this one paints arrow's own chrome (topbar, sidebar,
// Sessions, modals) by stamping `data-theme` on <html>, which app.css keys off.
//
// Deliberately light/dark only, no "system": inside a Tauri window on Linux
// `prefers-color-scheme` tracks whether the GTK theme HAS a light variant, not the
// user's color-scheme preference — under a dark-only theme (Pop!_OS's default) it stays
// pinned to dark and fires no change event, so a "System" mode built on it would be
// silently wrong on arrow's primary platform. Following the OS honestly needs a polled
// read of the XDG portal through Tauri; see ROADMAP.
import { THEMES } from './themes'

export type Appearance = 'dark' | 'light'

const KEY = 'arrow.appearance'
/** The CodeMirror theme key, read once to seed a sensible first-run default. */
const EDITOR_THEME_KEY = 'arrow.theme'

/** Dark is the default: it is what arrow shipped before this setting existed. */
export const DEFAULT_APPEARANCE: Appearance = 'dark'

/**
 * The stored appearance, or a first-run seed seeded from the editor theme the user
 * already picked. Someone running GitHub Light in the editor was the one staring at a
 * light diff inside a dark shell — starting them on the dark chrome would upgrade them
 * straight back into that mismatch.
 */
export function loadAppearance(): Appearance {
  const stored = localStorage.getItem(KEY)
  if (stored === 'light' || stored === 'dark') return stored

  const editorTheme = localStorage.getItem(EDITOR_THEME_KEY)
  const match = THEMES.find((t) => t.id === editorTheme)
  return match ? (match.dark ? 'dark' : 'light') : DEFAULT_APPEARANCE
}

/** Persists the choice and stamps <html data-theme>, which swaps the tokens in app.css. */
export function applyAppearance(a: Appearance): void {
  localStorage.setItem(KEY, a)
  document.documentElement.dataset.theme = a
}
