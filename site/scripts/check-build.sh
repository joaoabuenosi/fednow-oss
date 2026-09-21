#!/usr/bin/env bash
#
# check-build.sh — enforce the two trademark rules that a human reviewer cannot
# be expected to re-check on every page, on every build.
#
#   1. "FedNow" must NEVER appear inside <head> of any built page. The mark's
#      terms forbid using it as a metatag or hidden text, so it must stay out of
#      <title>, meta description/keywords, Open Graph and Twitter tags, JSON-LD
#      and canonical URLs. Everything in <head> is, by definition, metadata.
#   2. The non-affiliation footer must appear on EVERY built page, including the
#      404 page.
#
# It also checks that the mark is absent from URLs/slugs, and that no analytics
# or cookie-setting script slipped in.
#
# Usage: npm run build && npm run check
set -uo pipefail

DIST="${1:-dist}"
MARK='fednow'
FOOTER='Not affiliated with, endorsed by or sponsored by the Federal Reserve.'
FOOTER_MARK='FedNow is a service mark of the Federal Reserve Banks.'

if [ ! -d "$DIST" ]; then
  echo "FAIL: $DIST does not exist — run 'npm run build' first." >&2
  exit 1
fi

mapfile -t PAGES < <(find "$DIST" -name '*.html' -type f | sort)
if [ "${#PAGES[@]}" -eq 0 ]; then
  echo "FAIL: no HTML pages found under $DIST." >&2
  exit 1
fi

echo "Checking ${#PAGES[@]} built pages under $DIST/"
echo

status=0

# ---------------------------------------------------------------- 1. <head> --
# awk with RS="</head>" yields everything before the first </head>: the doctype,
# the <html> tag and the whole <head>. grep then looks for the mark in it.
echo "[1/5] No \"FedNow\" inside <head> (metatags, OG/Twitter, JSON-LD, canonical)"
head_hits=0
for page in "${PAGES[@]}"; do
  if awk 'BEGIN { RS = "</head>" } NR == 1' "$page" | grep -i -q "$MARK"; then
    echo "      FAIL: ${page#"$DIST"/}"
    awk 'BEGIN { RS = "</head>" } NR == 1' "$page" | grep -i -o ".\{0,60\}$MARK.\{0,60\}" | sed 's/^/             /'
    head_hits=$((head_hits + 1))
  fi
done
if [ "$head_hits" -eq 0 ]; then
  echo "      PASS: 0 of ${#PAGES[@]} pages contain the mark inside <head>"
else
  echo "      FAIL: $head_hits page(s) contain the mark inside <head>"
  status=1
fi
echo

# --------------------------------------------------------------- 2. footer --
echo "[2/5] Non-affiliation footer present on every page"
missing=0
for page in "${PAGES[@]}"; do
  if ! grep -qF "$FOOTER" "$page" || ! grep -qF "$FOOTER_MARK" "$page"; then
    echo "      FAIL: ${page#"$DIST"/}"
    missing=$((missing + 1))
  fi
done
if [ "$missing" -eq 0 ]; then
  echo "      PASS: ${#PAGES[@]} of ${#PAGES[@]} pages carry the footer"
else
  echo "      FAIL: $missing page(s) missing the footer"
  status=1
fi
echo

# ----------------------------------------------------------------- 3. URLs --
echo "[3/5] No \"FedNow\" in URLs or slugs"
if slugs=$(find "$DIST" -type f | grep -i "$MARK"); then
  echo "      FAIL: built paths contain the mark:"
  echo "$slugs" | sed 's/^/             /'
  status=1
else
  echo "      PASS: no built path contains the mark"
fi
echo

# ------------------------------------------------------------ 4. analytics --
echo "[4/5] No analytics or third-party trackers"
if trackers=$(grep -rl -E 'googletagmanager|google-analytics|gtag\(|plausible\.io|analytics\.js|/_vercel/insights' "$DIST" 2>/dev/null); then
  echo "      FAIL: tracker references found:"
  echo "$trackers" | sed 's/^/             /'
  status=1
else
  echo "      PASS: none found (see the TODO in astro.config.mjs for cookieless analytics)"
fi
echo

# ------------------------------------------------- 5. unresolved MDX exprs --
# MDX does not evaluate `{...}` in a markdown link destination; such a link is
# emitted verbatim and URL-encoded (%7B...%7D). Catch it rather than ship it.
echo "[5/5] No unresolved MDX expressions in links"
if broken=$(grep -rlE 'href="%7B|href="\{|src="%7B' "$DIST" --include='*.html' 2>/dev/null); then
  echo "      FAIL: unevaluated expressions in href/src:"
  echo "$broken" | sed 's/^/             /'
  status=1
else
  echo "      PASS: none found"
fi
echo

if [ "$status" -eq 0 ]; then
  echo "All checks passed."
else
  echo "Checks FAILED." >&2
fi
exit "$status"
