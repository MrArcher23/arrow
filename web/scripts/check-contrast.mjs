// Contrast gate for the appearance palettes in src/app.css.
//
// There is no frontend test runner in this project, so this script is the only mechanical
// check on the thing most likely to regress silently: a token edited in one polarity and
// not the other, or a light value that quietly drops arrow's honesty signals (the `live`
// dot, the ⚠ userModified marker, the expiry countdown) below readability.
//
// It parses the two palette blocks straight out of app.css — no duplicated color table to
// drift — and asserts WCAG 2 contrast on the pairs that actually occur in the components.
//
//   node scripts/check-contrast.mjs
//
// Exits non-zero on any failure.
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const CSS = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'app.css')

/** Text needs 4.5:1 (WCAG AA); non-text UI that carries state needs 3:1. */
const TEXT_MIN = 4.5
const UI_MIN = 3.0

// Pairs verified against the real components. NOTE: an earlier version of this file
// assumed the semantic colors only ever sit on --bg / --panel. That is FALSE, and the
// assumption hid four real defects — a row repaints to --hover the moment you point at
// it, which is exactly when Sidebar's ⚠ flag, SessionsView's expiry countdown and
// WorktreesModal's row flags need to stay readable. Every ground a color actually
// touches is enumerated here.
const TEXT_PAIRS = [
  ['fg', 'bg'], ['fg', 'panel'], ['fg', 'hover'], ['fg', 'active'], ['fg', 'chip'],
  ['dim', 'bg'], ['dim', 'panel'], ['dim', 'hover'], ['dim', 'active'], ['dim', 'chip'],
  // Semantic text on every ground it lands on, hover/selection included.
  ['accent', 'bg'], ['accent', 'panel'], ['accent', 'hover'], ['accent', 'active'], ['accent', 'chip'],
  ['green', 'bg'], ['green', 'panel'], ['green', 'hover'], ['green', 'active'], ['green', 'chip'],
  ['red', 'bg'], ['red', 'panel'], ['red', 'hover'], ['red', 'active'], ['red', 'chip'],
  ['warn', 'bg'], ['warn', 'panel'], ['warn', 'hover'], ['warn', 'active'], ['warn', 'chip'],
  // Banner text on its own tinted ground.
  ['green', 'add-bg'], ['red', 'del-bg'], ['warn', 'warn-bg'],
  // Chip/button labels drawn on a solid semantic fill.
  ['on-accent', 'green'], ['on-accent', 'warn'], ['on-accent', 'red'], ['on-accent', 'accent'],
  // ...and on the muted fills used for in-flight and hover states. These exist because a
  // blanket `opacity` would composite the label INTO the fill; the gate must cover them
  // or the states that replaced the opacity go unchecked.
  ['on-accent', 'warn-dim'], ['on-accent', 'green-hover'],
]

// Non-text, but state-bearing: the `live` dot has to stay findable against its ground.
const UI_PAIRS = [['green', 'bg'], ['green', 'panel']]

// Pre-existing debt, recorded rather than hidden. --dim in the DARK palette has shipped
// below AA since before this gate existed; the light palette clears it comfortably
// (5.2-6.1:1). These entries pin today's ratios as a floor: the gate still fails if any
// of them gets WORSE, so the debt cannot deepen silently. Raising --dim to a passing
// value is a deliberate visual change to a released theme — see ROADMAP.
const KNOWN_BELOW_AA = {
  'dark:dim/bg': 4.12,
  'dark:dim/panel': 3.95,
  'dark:dim/hover': 3.43,
  'dark:dim/active': 3.38,
  'dark:dim/chip': 3.43,
}

function parsePalettes(rawCss) {
  // Strip comments first: the palette comments legitimately mention token names like
  // `--warn-dim:`, and the declaration regex below would otherwise match inside them and
  // swallow the real declaration that follows.
  const css = rawCss.replace(/\/\*[\s\S]*?\*\//g, '')
  const block = (selector) => {
    const i = css.indexOf(selector)
    if (i === -1) throw new Error(`palette block not found: ${selector}`)
    const body = css.slice(css.indexOf('{', i) + 1, css.indexOf('}', i))
    const out = {}
    for (const [, name, value] of body.matchAll(/--([a-z-]+):\s*([^;]+);/g)) {
      const v = value.trim()
      if (/^#[0-9a-f]{3,8}$/i.test(v)) out[name] = v
    }
    return out
  }
  const dark = block(':root {')
  // The light block only redefines tokens; anything it omits falls through to :root.
  const light = { ...dark, ...block(':root[data-theme="light"] {') }
  return { dark, light }
}

function toRgb(hex) {
  let h = hex.slice(1)
  if (h.length === 3) h = [...h].map((c) => c + c).join('')
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16))
}

function luminance(hex) {
  const [r, g, b] = toRgb(hex).map((v) => {
    const s = v / 255
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrast(a, b) {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p)
  return (x + 0.05) / (y + 0.05)
}

const { dark, light } = parsePalettes(readFileSync(CSS, 'utf8'))
let failed = 0

for (const [label, palette] of [['dark', dark], ['light', light]]) {
  console.log(`\n${label} palette`)
  const check = (fg, bg, min, kind) => {
    if (!palette[fg] || !palette[bg]) {
      console.log(`  ?? --${fg} on --${bg}: token missing`)
      failed++
      return
    }
    const ratio = contrast(palette[fg], palette[bg])
    const floor = KNOWN_BELOW_AA[`${label}:${fg}/${bg}`]
    // A known-below-AA pair is held to its recorded ratio instead of the AA threshold.
    const effectiveMin = floor !== undefined ? floor - 0.005 : min
    const ok = ratio >= effectiveMin
    if (!ok) failed++
    const mark = ok ? (floor !== undefined ? 'debt' : 'ok  ') : 'FAIL'
    const needs = floor !== undefined ? `known debt, floor ${floor}` : `${kind} needs ${min}`
    console.log(`  ${mark} --${fg} on --${bg}: ${ratio.toFixed(2)}:1 (${needs})`)
  }
  for (const [fg, bg] of TEXT_PAIRS) check(fg, bg, TEXT_MIN, 'text')
  for (const [fg, bg] of UI_PAIRS) check(fg, bg, UI_MIN, 'ui')
}

if (failed) {
  console.error(`\n${failed} contrast check(s) failed.`)
  process.exit(1)
}
console.log('\nAll contrast checks passed.')
