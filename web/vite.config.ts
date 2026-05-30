import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { execFile } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import type { IncomingMessage, ServerResponse } from 'node:http'

const __dirname = dirname(fileURLToPath(import.meta.url))
// El binario del parser de Rust. En Fase 2 (Tauri) esto se reemplaza por comandos invoke().
const BIN = resolve(__dirname, '../target/release/arrow')
const MAX = 128 * 1024 * 1024 // los transcripts grandes producen JSON grande

function run(args: string[], res: ServerResponse) {
  execFile(BIN, args, { maxBuffer: MAX }, (err, stdout, stderr) => {
    res.setHeader('content-type', 'application/json; charset=utf-8')
    if (err) {
      res.statusCode = 500
      res.end(JSON.stringify({ error: String(stderr || err.message) }))
      return
    }
    res.end(stdout)
  })
}

// Plugin de dev que expone el parser como API HTTP local.
function arrowApi() {
  return {
    name: 'arrow-api',
    configureServer(server: any) {
      server.middlewares.use('/api/report', (_req: IncomingMessage, res: ServerResponse) => {
        run(['--json'], res)
      })
      server.middlewares.use('/api/content', (req: IncomingMessage, res: ServerResponse) => {
        const url = new URL(req.url ?? '', 'http://localhost')
        const file = url.searchParams.get('file') ?? ''
        const session = url.searchParams.get('session') ?? ''
        if (!file) {
          res.statusCode = 400
          res.end(JSON.stringify({ error: 'falta el parámetro file' }))
          return
        }
        const args = ['--content', '--file', file]
        if (session) args.push('--session', session)
        run(args, res)
      })
    },
  }
}

export default defineConfig({
  plugins: [svelte(), arrowApi()],
  server: { port: 5173 },
})
