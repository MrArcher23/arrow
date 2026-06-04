#!/usr/bin/env bash
#
# arrow — one-line macOS installer.
#
#   /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/MrArcher23/arrow/main/install.sh)"
#
# Downloads the latest published .dmg for this Mac's architecture from the GitHub Release,
# copies arrow.app into /Applications, and removes the Gatekeeper quarantine attribute (the
# app is unsigned). It only fetches what `release.yml` already published — it builds nothing.
#
# Honest limits (this is a plain installer, not a package manager):
#   - No auto-update: to update, re-run this one-liner.
#   - No uninstaller: to remove, `rm -rf /Applications/arrow.app`.
#   - The app is unsigned/un-notarized; this script strips the quarantine flag so it opens
#     without the right-click → Open dance. That means telling macOS to trust an unsigned
#     binary — only run this if you trust the source.

set -euo pipefail

REPO="MrArcher23/arrow"
APP_NAME="arrow.app"
INSTALL_DIR="/Applications"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }
info() { printf '==> %s\n' "$1"; }

# --- 1. Platform & architecture -------------------------------------------------------------
[ "$(uname -s)" = "Darwin" ] || err "this installer is for macOS only (detected $(uname -s)). On Linux use the .deb / AppImage from the Releases page."

case "$(uname -m)" in
  arm64)  ARCH_SUFFIX="aarch64" ;;  # Apple Silicon
  x86_64) ARCH_SUFFIX="x64" ;;      # Intel
  *)      err "unsupported architecture: $(uname -m)" ;;
esac
info "macOS / $(uname -m) → looking for an '${ARCH_SUFFIX}' .dmg"

# --- 2. Resolve the latest Release's .dmg for this architecture ------------------------------
# The asset name carries the version (e.g. arrow_0.1.4_aarch64.dmg), so the
# /releases/latest/download/<name> shortcut can't be hardcoded — query the API instead.
API_URL="https://api.github.com/repos/${REPO}/releases/latest"
info "querying the latest release of ${REPO}…"
RELEASE_JSON="$(curl -fsSL -H "Accept: application/vnd.github+json" "$API_URL")" \
  || err "could not reach the GitHub API. Check your connection or try again later."

# Pick the browser_download_url of the .dmg whose name contains our arch suffix.
# The trailing `|| true` is REQUIRED: under `set -euo pipefail`, a no-match `grep` exits 1,
# which would abort the script AT this assignment — before the friendly guard below could run,
# leaving the user with a bare `exit 1` and no message (the common case until the first macOS
# .dmg is published). `|| true` lets the assignment succeed with an empty value so the explicit
# check on the next line emits the actionable error instead.
DMG_URL="$(printf '%s' "$RELEASE_JSON" \
  | grep -o '"browser_download_url": *"[^"]*"' \
  | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/' \
  | grep "_${ARCH_SUFFIX}\.dmg$" \
  | head -n 1 || true)"

[ -n "$DMG_URL" ] || err "no .dmg for '${ARCH_SUFFIX}' in the latest release. It may not be published yet — see https://github.com/${REPO}/releases"

# --- 3. Download to a temp dir --------------------------------------------------------------
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
DMG_PATH="${TMP_DIR}/arrow.dmg"
info "downloading $(basename "$DMG_URL")…"
curl -fsSL "$DMG_URL" -o "$DMG_PATH" || err "download failed."

# --- 4. Mount, copy into /Applications, detach ----------------------------------------------
info "mounting the disk image…"
MOUNT_POINT="$(mktemp -d "${TMP_DIR}/mount.XXXXXX")"
hdiutil attach "$DMG_PATH" -nobrowse -quiet -mountpoint "$MOUNT_POINT" \
  || err "could not mount the .dmg."
# Always detach the volume, even if the copy below fails.
trap 'hdiutil detach "$MOUNT_POINT" -quiet >/dev/null 2>&1 || true; rm -rf "$TMP_DIR"' EXIT

SRC_APP="${MOUNT_POINT}/${APP_NAME}"
[ -d "$SRC_APP" ] || err "the .dmg did not contain ${APP_NAME}."

DEST_APP="${INSTALL_DIR}/${APP_NAME}"
if [ -d "$DEST_APP" ]; then
  info "removing the previous install at ${DEST_APP}…"
  rm -rf "$DEST_APP"
fi
info "installing to ${DEST_APP}…"
cp -R "$SRC_APP" "$INSTALL_DIR/" || err "could not copy into ${INSTALL_DIR} (permissions?)."

# --- 5. Gatekeeper: strip the quarantine flag (app is unsigned) -----------------------------
info "removing the Gatekeeper quarantine flag (arrow is unsigned — only do this if you trust the source)…"
xattr -dr com.apple.quarantine "$DEST_APP" 2>/dev/null || true

# --- 6. Done --------------------------------------------------------------------------------
info "arrow installed in ${INSTALL_DIR}. Launch it from Applications or run: open '${DEST_APP}'"
