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
#   3. No real ABA routing number may be published. The site republishes the
#      repository's own markdown, so a real institution's routing number pasted
#      into a doc would be served from here, next to the mark — which reads as
#      that institution participating or endorsing. Every example identifier in
#      this project is fictitious and lives in the unassignable 99 block; see
#      docs/handbook/README.md.
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
echo "[1/6] No \"FedNow\" inside <head> (metatags, OG/Twitter, JSON-LD, canonical)"
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
echo "[2/6] Non-affiliation footer present on every page"
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
echo "[3/6] No \"FedNow\" in URLs or slugs"
if slugs=$(find "$DIST" -type f | grep -i "$MARK"); then
  echo "      FAIL: built paths contain the mark:"
  echo "$slugs" | sed 's/^/             /'
  status=1
else
  echo "      PASS: no built path contains the mark"
fi
echo

# ------------------------------------------------------------ 4. analytics --
echo "[4/6] No analytics or third-party trackers"
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
echo "[5/6] No unresolved MDX expressions in links"
if broken=$(grep -rlE 'href="%7B|href="\{|src="%7B' "$DIST" --include='*.html' 2>/dev/null); then
  echo "      FAIL: unevaluated expressions in href/src:"
  echo "$broken" | sed 's/^/             /'
  status=1
else
  echo "      PASS: none found"
fi
echo

# ------------------------------------------------ 6. real routing numbers --
# Values retired from this repository because they are, or could plausibly be, a
# real institution's ABA routing number. 091000019 is Wells Fargo Bank NA. The
# 0210xxxxx pair sit in an assignable New York range with valid check digits,
# which is exactly what makes them unsafe to publish. Replacements all begin
# with 99, a block the ABA assigns to nobody.
echo "[6/6] No known-real ABA routing numbers in the published output"
DENYLIST="091000019 021040078 021150706 021040079 091000018"
rtn_hits=0
for rtn in $DENYLIST; do
  if hits=$(grep -rl "$rtn" "$DIST" 2>/dev/null); then
    echo "      FAIL: $rtn appears in:"
    echo "$hits" | sed 's/^/             /'
    rtn_hits=$((rtn_hits + 1))
  fi
done
if [ "$rtn_hits" -eq 0 ]; then
  echo "      PASS: none of the $(set -- $DENYLIST; echo $#) retired routing numbers appear in $DIST/"
else
  echo "      FAIL: $rtn_hits retired routing number(s) published"
  status=1
fi
echo

if [ "$status" -eq 0 ]; then
  echo "All checks passed."
else
  echo "Checks FAILED." >&2
fi
exit "$status"
