// The app version, injected at build time by Vite's `define` (read from
// src-tauri/tauri.conf.json — the version that stamps the shipped bundle). Kept
// in its own module so components import a clean constant and never touch the
// define directly — same isolation convention as zoom.ts / window.ts.
export const appVersion: string = __APP_VERSION__

// The project's releases page (for the About popover).
export const RELEASES_URL = 'https://github.com/MrArcher23/arrow/releases/latest'
