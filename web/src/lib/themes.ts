import type { Extension } from '@codemirror/state'
import {
  githubDark,
  githubLight,
  dracula,
  tokyoNight,
  vscodeDark,
  nord,
  monokai,
  materialDark,
  gruvboxDark,
  atomone,
  aura,
  androidstudio,
  sublime,
  solarizedLight,
  basicLight,
  bbedit,
  consoleLight,
  duotoneLight,
  eclipse,
  gruvboxLight,
  materialLight,
  noctisLilac,
  quietlight,
  tokyoNightDay,
  vscodeLight,
  whiteLight,
  xcodeLight,
} from '@uiw/codemirror-themes-all'

export interface ThemeDef {
  id: string
  label: string
  dark: boolean
  ext: Extension
}

// Temas de la comunidad uiw (extensiones CM6 agnósticas del framework).
// These theme the EDITOR only. The app's own chrome is themed separately — see
// lib/appearance.ts — so a light chrome and a dark editor (or vice versa) is a valid
// combination the user can choose.
export const THEMES: ThemeDef[] = [
  { id: 'githubDark', label: 'GitHub Dark', dark: true, ext: githubDark },
  { id: 'dracula', label: 'Dracula', dark: true, ext: dracula },
  { id: 'tokyoNight', label: 'Tokyo Night', dark: true, ext: tokyoNight },
  { id: 'vscodeDark', label: 'VS Code Dark', dark: true, ext: vscodeDark },
  { id: 'nord', label: 'Nord', dark: true, ext: nord },
  { id: 'monokai', label: 'Monokai', dark: true, ext: monokai },
  { id: 'materialDark', label: 'Material Dark', dark: true, ext: materialDark },
  { id: 'gruvboxDark', label: 'Gruvbox Dark', dark: true, ext: gruvboxDark },
  { id: 'atomone', label: 'Atom One', dark: true, ext: atomone },
  { id: 'aura', label: 'Aura', dark: true, ext: aura },
  { id: 'androidstudio', label: 'Android Studio', dark: true, ext: androidstudio },
  { id: 'sublime', label: 'Sublime', dark: true, ext: sublime },
  { id: 'githubLight', label: 'GitHub Light', dark: false, ext: githubLight },
  { id: 'solarizedLight', label: 'Solarized Light', dark: false, ext: solarizedLight },
  { id: 'vscodeLight', label: 'VS Code Light', dark: false, ext: vscodeLight },
  { id: 'materialLight', label: 'Material Light', dark: false, ext: materialLight },
  { id: 'gruvboxLight', label: 'Gruvbox Light', dark: false, ext: gruvboxLight },
  { id: 'tokyoNightDay', label: 'Tokyo Night Day', dark: false, ext: tokyoNightDay },
  { id: 'duotoneLight', label: 'Duotone Light', dark: false, ext: duotoneLight },
  { id: 'quietlight', label: 'Quiet Light', dark: false, ext: quietlight },
  { id: 'noctisLilac', label: 'Noctis Lilac', dark: false, ext: noctisLilac },
  { id: 'xcodeLight', label: 'Xcode Light', dark: false, ext: xcodeLight },
  { id: 'eclipse', label: 'Eclipse', dark: false, ext: eclipse },
  { id: 'bbedit', label: 'BBEdit', dark: false, ext: bbedit },
  { id: 'basicLight', label: 'Basic Light', dark: false, ext: basicLight },
  { id: 'consoleLight', label: 'Console Light', dark: false, ext: consoleLight },
  { id: 'whiteLight', label: 'White', dark: false, ext: whiteLight },
]

export const DEFAULT_THEME = 'githubDark'

const byId = new Map(THEMES.map((t) => [t.id, t]))

export function themeExt(id: string): Extension {
  return (byId.get(id) ?? byId.get(DEFAULT_THEME)!).ext
}
