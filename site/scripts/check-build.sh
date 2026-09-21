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
#   4. The social preview image is metadata too, and it travels further than any
#      page: an og:image is what gets pasted into a chat, a timeline or a deck,
#      usually with none of the surrounding text. The mark must therefore be
#      absent from the og:/twitter: tags, from the alt text they carry, and from
#      the bytes of the image files themselves. Check [1/9] covers og:/twitter:
#      tags only while they sit inside <head>; [7/9] and [8/9] cover the rest,
#      which is the half a <head> grep cannot see.
#
# It also checks that the mark is absent from URLs/slugs, and that the only
# analytics on the site is the cookieless first-party one the pages claim.
#
# Usage: npm run build && npm run check
set -uo pipefail

DIST="${1:-dist}"
MARK='fednow'

# What actually ships to a browser. A check about "what the site DOES when
# loaded" must scan all of it: Astro emits client JS into dist/_astro/*.js, so a
# tracker imported by a component — exactly what the @vercel/analytics package
# would do — lands in a bundle and never appears in the page HTML. Scoping the
# tracker grep to *.html was a real hole, found in review of ccf60d9.
#
# The converse is also true and is not an oversight: checks about page STRUCTURE
# or about what the MARKDOWN AUTHOR wrote (<head>, the footer, the social tags,
# unevaluated MDX expressions, edit links) stay on the HTML pages. Widening
# those to JS produces false positives from third-party bundles that legitimately
# contain the same syntax — Pagefind's search UI ships a client-side template
# whose markup contains href="{{ ... }}", which is a Mustache placeholder it
# evaluates at runtime, not an Astro expression that failed to evaluate at build
# time. Each check below says which of the two it is.
SHIPPED=(--include='*.html' --include='*.js' --include='*.css')

# Overridable so scripts/selftest-checks.sh can point check [8/9] at a fixture.
OG_SVG="${OG_SVG:-src/assets/og-image.svg}"
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
echo "[1/9] No \"FedNow\" inside <head> (metatags, OG/Twitter, JSON-LD, canonical)"
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
echo "[2/9] Non-affiliation footer present on every page"
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
# Paths are matched with the $DIST prefix STRIPPED. Without that, running the
# check against an absolute path fails on the path itself — this repository is
# called fednow-oss, so `check-build.sh /home/me/fednow-oss/site/dist` would
# report all 18 pages as violations. What is being checked is the published URL
# space, which is what is left after the prefix comes off.
echo "[3/9] No \"FedNow\" in URLs or slugs"
if slugs=$(find "$DIST" -type f | sed "s|^$DIST/||" | grep -i "$MARK"); then
  echo "      FAIL: built paths contain the mark:"
  echo "$slugs" | sed 's/^/             /'
  status=1
else
  echo "      PASS: no built path contains the mark"
fi
echo

# ------------------------------------------------------------ 4. analytics --
# The site carries exactly one analytics script: Vercel Web Analytics, loaded
# first-party from /_vercel/insights/script.js. Three things have to hold, and
# all three are things a future edit could quietly break:
#
#   a. No third-party tracker. Anything that reaches a host other than this one
#      breaks the "no third-party requests" half of the claim on /evaluate/.
#   b. The insights script stays FIRST-PARTY — a root-relative path, never an
#      absolute URL and never va.vercel-scripts.com, which is the debug script
#      @vercel/analytics swaps in outside production.
#   c. No cookies, at all, from anything. And no browser storage beyond the two
#      keys below, which are declared rather than discovered: Starlight keeps
#      the visitor's light/dark choice in localStorage and which sidebar groups
#      are open in sessionStorage. Both are UI preferences that never leave the
#      browser and identify nobody — but the site says so out loud on /about/
#      rather than rounding "no tracking" up to "no storage", so a third key
#      appearing means either the prose is now wrong or something was added
#      that should not have been. Either way, that is a build failure.
#
# And the reason this check exists in this shape at all: /evaluate/, /about/ and
# robots.txt each make a specific, narrow privacy claim. The last block below
# fails the build if the analytics script is present while a page still carries
# the old "loads no analytics" wording — the site must never make a false claim,
# and the way that would happen is by adding the script and forgetting the prose
# (or removing the prose and forgetting the script).
# SCOPE: everything that ships (html + js + css). This one is about behaviour.
echo "[4/9] Analytics: first-party and cookieless, or nothing at all"

third_party='googletagmanager|google-analytics|gtag\(|plausible\.io|analytics\.js|segment\.com|hotjar|matomo|va\.vercel-scripts\.com'
if trackers=$(grep -rlE "$third_party" "$DIST" "${SHIPPED[@]}" 2>/dev/null); then
  echo "      FAIL: third-party tracker references found:"
  echo "$trackers" | sed 's/^/             /'
  status=1
else
  echo "      PASS: no third-party tracker referenced"
fi

# Any reference to the insights endpoint must be root-relative, i.e. served from
# this origin. An absolute URL would make it a third-party request.
if offsite=$(grep -rhoE '(src|href)="[^"]*_vercel/insights[^"]*"' "$DIST" "${SHIPPED[@]}" 2>/dev/null \
             | sort -u | grep -v '="/_vercel/insights'); then
  echo "      FAIL: insights loaded from somewhere other than this origin:"
  echo "$offsite" | sed 's/^/             /'
  status=1
else
  echo "      PASS: every /_vercel/insights reference is first-party (root-relative)"
fi

if cookies=$(grep -rl 'document\.cookie' "$DIST" "${SHIPPED[@]}" 2>/dev/null); then
  echo "      FAIL: a script touches document.cookie:"
  echo "$cookies" | sed 's/^/             /'
  status=1
else
  echo "      PASS: nothing on any page touches document.cookie"
fi

# Storage keys the site declares on /about/. Anything else is undeclared.
DECLARED_STORAGE_KEYS='starlight-theme sl-sidebar-state'
key_pattern="$(echo "$DECLARED_STORAGE_KEYS" | tr ' ' '|')"
storage_ok=1
for key in $DECLARED_STORAGE_KEYS; do
  grep -rqF "$key" "$DIST" "${SHIPPED[@]}" 2>/dev/null || {
    echo "      NOTE: declared storage key \"$key\" is no longer used — /about/ can drop it"
  }
done
# The claim on /about/ names these two and no others. Only string literals are
# matched: the bundler minifies key names into variables in plenty of places,
# and a bare identifier says nothing about what is actually stored. A literal
# key that is not on the list is the case worth failing on, and it is also the
# only way a NEW piece of stored state realistically arrives.
# The closing backreference MUST be \3 — the quote-character group. It was \2
# (the set|get group), so the pattern only matched a key literally ending in
# "set" or "get", i.e. never, and this guard printed PASS unconditionally on
# every input. Caught in review on ccf60d9; scripts/selftest-checks.sh now
# injects a stray key on every run so it cannot silently stop failing again.
if stray=$(grep -rhoE "(local|session)Storage\.(set|get)Item\(([\`\"'])[A-Za-z0-9_.-]+\3" "$DIST" "${SHIPPED[@]}" 2>/dev/null \
           | grep -oE '[A-Za-z0-9_.-]+.$' | sed 's/.$//' | sort -u | grep -vE "^($key_pattern)$"); then
  echo "      FAIL: undeclared browser-storage key(s) — /about/ names only: $DECLARED_STORAGE_KEYS"
  echo "$stray" | sed 's/^/             /'
  storage_ok=0
  status=1
fi
[ "$storage_ok" -eq 1 ] && echo "      PASS: browser storage limited to the declared UI-preference keys ($DECLARED_STORAGE_KEYS)"

# The claim and the code must agree, in whichever direction they disagree.
STALE_CLAIM='loads no analytics'
analytics_present=0
grep -rqF '/_vercel/insights/script.js' "$DIST" "${SHIPPED[@]}" 2>/dev/null && analytics_present=1
stale_pages=$(grep -rlF "$STALE_CLAIM" "$DIST" --include='*.html' 2>/dev/null || true)

if [ "$analytics_present" -eq 1 ] && [ -n "$stale_pages" ]; then
  echo "      FAIL: analytics is loaded, but these pages still claim the site \"$STALE_CLAIM\":"
  echo "$stale_pages" | sed 's/^/             /'
  echo "             Fix the prose (/evaluate/, /about/, robots.txt) — do not ship a false claim."
  status=1
elif [ "$analytics_present" -eq 0 ] && [ -z "$stale_pages" ]; then
  echo "      PASS: no analytics loaded, and no page claims otherwise"
else
  echo "      PASS: the analytics the pages describe is the analytics they load"
fi
echo

# ------------------------------------------------- 5. unresolved MDX exprs --
# MDX does not evaluate `{...}` in a markdown link destination; such a link is
# emitted verbatim and URL-encoded (%7B...%7D). Catch it rather than ship it.
# SCOPE: HTML pages. This is about what the markdown author wrote surviving into
# the rendered page, not about runtime behaviour — and client bundles carry other
# template syntaxes that look identical (see the SHIPPED note above).
echo "[5/9] No unresolved MDX expressions in links"
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
echo "[6/9] No known-real ABA routing numbers in the published output"
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

# ------------------------------------------- 7. social / image metadata --
# Check [1/9] greps <head>, which is where these tags live today — but "today"
# is doing a lot of work in that sentence. An og:image can be emitted from a
# component, a <noscript> block or an integration that appends to <body>, and
# alt text is never in <head> at all. Both are machine-readable metadata under
# the mark's terms, so both are checked wherever they appear in the document.
echo "[7/9] No \"FedNow\" in og:/twitter: metadata or in any image alt text"
meta_hits=0
for page in "${PAGES[@]}"; do
  # Whole-document, not just <head>: every og:/twitter: meta tag, and every
  # alt="" on any element. -o keeps the report to the offending tag.
  if offenders=$(grep -ioE '<meta[^>]*(property|name)="(og|twitter):[^"]*"[^>]*>|alt="[^"]*"' "$page" 2>/dev/null \
                 | grep -i "$MARK"); then
    echo "      FAIL: ${page#"$DIST"/}"
    echo "$offenders" | sed 's/^/             /'
    meta_hits=$((meta_hits + 1))
  fi
done
if [ "$meta_hits" -eq 0 ]; then
  echo "      PASS: 0 of ${#PAGES[@]} pages carry the mark in a social tag or alt text"
else
  echo "      FAIL: $meta_hits page(s) carry the mark in a social tag or alt text"
  status=1
fi

# The tags are worth nothing if they point at an image that is not there: a
# missing og:image is a bare text card, which is the thing this replaced.
og_refs=$(grep -rhoE '<meta[^>]*property="og:image"[^>]*content="[^"]*"' "$DIST" --include='*.html' 2>/dev/null \
          | grep -oE 'content="[^"]*"' | cut -d'"' -f2 | sort -u)
if [ -z "$og_refs" ]; then
  echo "      FAIL: no og:image tag found on any page"
  status=1
else
  og_missing=0
  for url in $og_refs; do
    # The tag must be absolute (crawlers do not resolve relative og:image), and
    # the file it names must exist in the build.
    case "$url" in
      http://*|https://*) ;;
      *) echo "      FAIL: og:image \"$url\" is not an absolute URL"; og_missing=$((og_missing + 1)); continue ;;
    esac
    file="$DIST/${url#*://*/}"
    if [ ! -f "$file" ]; then
      echo "      FAIL: og:image \"$url\" is not in the build (looked for ${file#"$DIST"/})"
      og_missing=$((og_missing + 1))
    fi
  done
  if [ "$og_missing" -eq 0 ]; then
    echo "      PASS: every og:image is an absolute URL pointing at a file in the build"
  else
    status=1
  fi
fi
echo

# -------------------------------------------- 8. the preview image itself --
# The last place the mark could hide is inside the artwork: rendered as text in
# the picture, left in the SVG source, or carried in a PNG text chunk (tEXt,
# iTXt and zTXt are all plain enough to grep, and Chromium writes none of them
# — so any hit is something a later tool added). This greps the bytes of both
# files. A PNG is binary, hence -a.
#
# verify-og.mjs then re-reads the PNG: right dimensions for a large summary
# card, and artwork reaching the bottom edge rather than a silently cropped
# render. Neither is visible in a diff, and a wrong social card is only ever
# noticed by the person you were trying to impress.
echo "[8/9] The social preview image carries no mark, and is a valid 1200x630 card"
OG_PNG="$DIST/og-image.png"
img_status=0

for f in "$OG_SVG" "$OG_PNG"; do
  if [ ! -f "$f" ]; then
    echo "      FAIL: $f is missing"
    img_status=1
  elif grep -a -i -q "$MARK" "$f"; then
    echo "      FAIL: $f contains the mark"
    img_status=1
  fi
done
if [ "$img_status" -eq 0 ]; then
  echo "      PASS: neither $OG_SVG nor ${OG_PNG#"$DIST"/} contains the mark"
fi

if [ -f "$OG_PNG" ]; then
  if out=$(node scripts/verify-og.mjs "$OG_PNG" 2>&1); then
    echo "      PASS: ${out#verify-og: }"
  else
    echo "      FAIL: ${out#verify-og: FAIL — }"
    img_status=1
  fi
fi
[ "$img_status" -eq 0 ] || status=1
echo

# ------------------------------------------------- 9. "Edit page" links --
# Starlight builds an edit URL as its configured baseUrl plus the page's path
# relative to the ASTRO PROJECT ROOT (site/), not the repository root — so a
# hand-authored page silently produced `edit/main/src/content/docs/...`, which
# 404s, while every page synced by sync-docs.mjs was fine because it writes its
# own editUrl. Nothing failed; the links just did not work, and the only way
# anyone finds that out is by clicking one.
#
# So: resolve every edit link in the build back to a file in the working tree.
# The build runs from a full checkout (sync-docs.mjs already insists on it), so
# the repository root is one level up and the file either exists or it does not.
# A page with no edit link at all is fine — /, /about/ and /evaluate/ drop
# theirs on purpose, because they are the project's statements rather than docs
# to crowd-edit.
echo "[9/9] Every \"Edit page\" link resolves to a file in the repository"
EDIT_PREFIX="https://github.com/joaoabuenosi/fednow-oss/edit/main/"
REPO_ROOT="$(cd .. && pwd)"
edit_bad=0
edit_seen=0
while IFS= read -r url; do
  [ -n "$url" ] || continue
  edit_seen=$((edit_seen + 1))
  rel="${url#"$EDIT_PREFIX"}"
  if [ "$rel" = "$url" ]; then
    echo "      FAIL: edit link does not point at this repository: $url"
    edit_bad=$((edit_bad + 1))
  elif [ ! -f "$REPO_ROOT/$rel" ]; then
    echo "      FAIL: edit link 404s — $rel does not exist in the repository"
    echo "             $url"
    edit_bad=$((edit_bad + 1))
  fi
done <<EOF
$(grep -rhoE "href=\"${EDIT_PREFIX}[^\"]*\"" "$DIST" --include='*.html' 2>/dev/null \
  | sed 's/^href="//; s/"$//' | sort -u)
EOF

if [ "$edit_seen" -eq 0 ]; then
  echo "      FAIL: no \"Edit page\" link found on any page — the Build/docs pages should keep theirs"
  status=1
elif [ "$edit_bad" -eq 0 ]; then
  echo "      PASS: $edit_seen distinct edit link(s), every one resolving to a file in the repository"
else
  echo "      FAIL: $edit_bad of $edit_seen edit link(s) do not resolve"
  status=1
fi
echo

if [ "$status" -eq 0 ]; then
  echo "All checks passed."
else
  echo "Checks FAILED." >&2
fi
exit "$status"
