import { LanguageDescription } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import type { Extension } from '@codemirror/state'

// Resuelve el resaltado de sintaxis para un archivo, cargando el parser del
// lenguaje BAJO DEMANDA (code-splitting). El bundle inicial no incluye gramáticas.
export async function resolveLanguage(path: string): Promise<Extension | null> {
  const name = path.split('/').pop() || path
  const desc = LanguageDescription.matchFilename(languages, name)
  if (!desc) return null
  try {
    return await desc.load()
  } catch {
    return null
  }
}
