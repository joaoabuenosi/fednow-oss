#!/usr/bin/env bash
#
# render-og.sh — render the social preview SVG to the PNG the site serves.
#
# Both files are committed: src/assets/og-image.svg is the source you edit, and
# public/og-image.png is what Open Graph and Twitter/X actually fetch (neither
# crawler renders SVG). This script is the one-liner that keeps them in step.
#
# It is NOT part of `npm run build` — it needs a Chromium binary, which the site
# does not otherwise depend on. Run it by hand after editing the SVG, and commit
# the regenerated PNG.
#
# Usage:
#   bash scripts/render-og.sh                       # finds a chromium on PATH
#   CHROMIUM=/path/to/chrome bash scripts/render-og.sh
#
# Two rendering quirks are worked around here, both of which silently crop the
# artwork rather than failing:
#
#   * A browser gives a standalone SVG document the default 8px body margin, so
#     the SVG is rendered inside a zero-margin HTML wrapper instead of opened
#     directly.
#   * Chromium's "new" headless mode lays the page out in a viewport shorter
#     than --window-size while still writing a screenshot of the full requested
#     size, leaving a flat band at the bottom. Playwright's headless_shell does
#     not, so it is preferred when both are present.
#
# Neither is left to trust: scripts/verify-og.mjs re-reads the PNG afterwards
# and fails if it is the wrong size or was cropped.
set -euo pipefail

SITE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$SITE_DIR/src/assets/og-image.svg"
OUT="$SITE_DIR/public/og-image.png"
WIDTH=1200
HEIGHT=630

if [ -n "${CHROMIUM:-}" ]; then
  BROWSER="$CHROMIUM"
else
  BROWSER=""
  for candidate in chromium chromium-browser google-chrome chrome; do
    if command -v "$candidate" >/dev/null 2>&1; then BROWSER="$(command -v "$candidate")"; break; fi
  done
  # Playwright keeps its browsers outside PATH; use one if it is installed.
  # headless_shell first, deliberately: see the --window-size note below.
  if [ -z "$BROWSER" ]; then
    for name in headless_shell chrome; do
      BROWSER="$(find "${PLAYWRIGHT_BROWSERS_PATH:-/opt/pw-browsers}" -maxdepth 3 -name "$name" -type f 2>/dev/null | head -n 1)"
      [ -n "$BROWSER" ] && break
    done
  fi
fi

if [ -z "$BROWSER" ] || [ ! -x "$BROWSER" ]; then
  echo "render-og: no Chromium found. Set CHROMIUM=/path/to/chrome and retry." >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$SRC" "$WORK/og.svg"
cat > "$WORK/og.html" <<HTML
<!doctype html><meta charset="utf-8">
<style>html,body{margin:0;padding:0;background:#0f1115}img{display:block}</style>
<img src="og.svg" width="$WIDTH" height="$HEIGHT" alt="">
HTML

"$BROWSER" --headless --disable-gpu --no-sandbox --hide-scrollbars \
  --force-device-scale-factor=1 \
  --screenshot="$OUT" --window-size="$WIDTH,$HEIGHT" \
  "file://$WORK/og.html" >/dev/null 2>&1

if [ ! -s "$OUT" ]; then
  echo "render-og: $BROWSER produced no output." >&2
  exit 1
fi

echo "render-og: wrote ${OUT#"$SITE_DIR"/} ($(wc -c < "$OUT") bytes) from ${SRC#"$SITE_DIR"/} using $BROWSER"
node "$SITE_DIR/scripts/verify-og.mjs" "$OUT"
