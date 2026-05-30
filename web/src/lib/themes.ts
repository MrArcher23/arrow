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
} from '@uiw/codemirror-themes-all'

export interface ThemeDef {
  id: string
  label: string
  dark: boolean
  ext: Extension
}

// Temas de la comunidad uiw (extensiones CM6 agnósticas del framework).
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
]

export const DEFAULT_THEME = 'githubDark'

const byId = new Map(THEMES.map((t) => [t.id, t]))

export function themeExt(id: string): Extension {
  return (byId.get(id) ?? byId.get(DEFAULT_THEME)!).ext
}
