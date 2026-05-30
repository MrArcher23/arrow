---
description: Recetas jq para inspeccionar el formato interno (no documentado) de los transcripts de Claude Code en ~/.claude/projects. Úsalo al extender el parser de arrow y necesitar confirmar la forma exacta de un campo antes de programar contra él.
allowed-tools: Bash(jq*) Bash(grep*) Bash(ls*) Bash(find*)
---

El JSONL de `~/.claude/projects/<dir>/<sessionId>.jsonl` es **interno y cambia entre versiones**.
NUNCA programes contra un campo sin confirmar su forma aquí primero. Extrae solo claves/estructura,
no contenido sensible de archivos ajenos.

Transcript más grande (para muestreo):
```
f=$(ls -S ~/.claude/projects/*/*.jsonl | head -1); echo "$f"
```

Distribución de tipos de record:
```
jq -r '.type' "$f" | sort | uniq -c | sort -rn
```

Forma del `toolUseResult` de un Edit/Write (sin volcar contenido):
```
grep -m1 structuredPatch "$f" | jq -c '.toolUseResult|keys'
grep -m1 structuredPatch "$f" | jq -c '.toolUseResult.structuredPatch[0]|{oldStart,oldLines,newStart,newLines,lines:(.lines[0:2])}'
```

Nombres de tool + claves de input (qué herramientas traen `file_path`):
```
jq -c 'select(.type=="assistant")|.message.content[]?|select(.type=="tool_use")|{name,ikeys:(.input|keys)}' "$f" | sort | uniq -c | sort -rn | head
```

Metadatos de sesión:
```
grep -m1 '"type":"ai-title"'    "$f" | jq -c '{keys:keys, aiTitle}'
grep -m1 '"type":"last-prompt"' "$f" | jq -c 'keys'
```

Snapshots del "antes" por archivo: `ls ~/.claude/file-history/<sessionId>/` → ficheros `<hash>@v<n>`
en texto plano (el dir está indexado por `sessionId`). El record `file-history-snapshot` mapea
`snapshot.trackedFileBackups` (ruta → backup).
