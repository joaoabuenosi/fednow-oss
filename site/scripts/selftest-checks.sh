#!/usr/bin/env bash
#
# selftest-checks.sh — prove that every check in check-build.sh can actually fail.
#
# WHY THIS EXISTS. Review of ccf60d9 found two checks in check-build.sh that
# reported PASS on every possible input:
#
#   * the browser-storage guard closed its regex with \2 (the set|get group)
#     instead of \3 (the quote group), so the pattern matched nothing, ever;
#   * the third-party-tracker grep was scoped to *.html, so a tracker inside
#     dist/_astro/*.js — precisely where a bundled analytics package would put
#     one — was invisible.
#
# Both were "green". Neither was checking anything. A check that cannot fail is
# worse than no check, because it is read as a guarantee: /evaluate/ and
# /about/ make narrow, specific privacy claims *on the strength of* these
# checks, and the whole premise is that the site can never state something that
# is not exactly true.
#
# So each case below takes the real build, injects one specific violation, and
# asserts THAT SPECIFIC CHECK reports FAIL. Asserting a non-zero exit code is
# not enough and was itself a bug during development: an unrelated check was
# failing for its own reason and every negative test looked like it passed.
#
# Run by `npm run check`, after check-build.sh has passed on the real build.
set -uo pipefail

SITE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SITE_DIR"

DIST="${1:-dist}"
TOTAL_CHECKS=9

if [ ! -d "$DIST" ]; then
  echo "FAIL: $DIST does not exist — run 'npm run build' first." >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

pass=0
fail=0

# Verdict of one numbered check in a check-build.sh run: FAIL if any FAIL line
# appears between its header and the next check's header.
verdict() { # $1 = output file, $2 = check number
  awk -v want="[$2/$TOTAL_CHECKS]" '
    index($0, want) == 1 { inblock = 1; next }
    /^\[[0-9]+\/[0-9]+\]/ { inblock = 0 }
    inblock && /FAIL/ { found = 1 }
    END { print (found ? "FAIL" : "PASS") }
  ' "$1"
}

# $1 = check number, $2 = description, $3 = shell mutating $D (and optionally
# exporting OG_SVG). The mutation runs against a throwaway copy of the build.
expect_fail() {
  local number="$1" description="$2" mutation="$3"
  local D="$WORK/dist" out="$WORK/out.txt"
  rm -rf "$D"; cp -r "$DIST" "$D"
  unset OG_SVG
  eval "$mutation" >/dev/null 2>&1
  bash scripts/check-build.sh "$D" >"$out" 2>&1
  local code=$? got
  got="$(verdict "$out" "$number")"
  if [ "$got" = "FAIL" ] && [ "$code" -ne 0 ]; then
    printf '  ok    [%d/%d]  %s\n' "$number" "$TOTAL_CHECKS" "$description"
    pass=$((pass + 1))
  else
    printf '  BROKEN[%d/%d]  %s\n' "$number" "$TOTAL_CHECKS" "$description"
    printf '        check [%d/%d] reported %s and the script exited %d — this violation went undetected.\n' \
      "$number" "$TOTAL_CHECKS" "$got" "$code"
    sed -n "/\[$number\/$TOTAL_CHECKS\]/,/^$/p" "$out" | sed 's/^/          /'
    fail=$((fail + 1))
  fi
}

echo "Self-testing $TOTAL_CHECKS checks: each violation below MUST be caught."
echo

# A positive control first. If the unmodified build does not pass cleanly,
# every "expected failure" below is meaningless.
if bash scripts/check-build.sh "$DIST" >"$WORK/base.txt" 2>&1; then
  echo "  ok    baseline  the unmodified build passes all $TOTAL_CHECKS checks"
  pass=$((pass + 1))
else
  echo "  BROKEN baseline the unmodified build does not pass — self-tests below prove nothing"
  fail=$((fail + 1))
fi
echo

MARK_TEXT='FedNow'

expect_fail 1 'the mark in <title>' \
  "sed -i 's|<title>|<title>$MARK_TEXT |' \"\$D/index.html\""

expect_fail 2 'a page missing the non-affiliation footer' \
  "sed -i 's|Not affiliated with, endorsed by or sponsored by the Federal Reserve\.||' \"\$D/about/index.html\""

expect_fail 3 'the mark in a published path' \
  "mkdir -p \"\$D/assets\" && echo x > \"\$D/assets/${MARK_TEXT,,}-leak.txt\""

expect_fail 4 'a third-party tracker in page HTML' \
  "sed -i 's|</head>|<script src=\"https://www.googletagmanager.com/gtag/js\"></script></head>|' \"\$D/about/index.html\""

expect_fail 4 'a third-party tracker inside a JS bundle' \
  "printf 'google-analytics' >> \$(find \"\$D/_astro\" -name '*.js' | head -1)"

expect_fail 4 'a third-party tracker inside a CSS bundle' \
  "printf '/*plausible.io*/' >> \$(find \"\$D/_astro\" -name '*.css' | head -1)"

expect_fail 4 'the insights script loaded from another origin' \
  "sed -i 's|src=\"/_vercel/insights/script.js\"|src=\"https://va.example.com/_vercel/insights/script.js\"|' \"\$D/about/index.html\""

expect_fail 4 'a script writing document.cookie' \
  "sed -i 's|</body>|<script>document.cookie=\"a=1\"</script></body>|' \"\$D/about/index.html\""

expect_fail 4 'an undeclared storage key in page HTML' \
  "sed -i 's|</body>|<script>localStorage.setItem(\"tracker-id\",\"x\")</script></body>|' \"\$D/index.html\""

expect_fail 4 'an undeclared storage key inside a JS bundle' \
  "printf 'sessionStorage.setItem(\"tracker-id\",1)' >> \$(find \"\$D/_astro\" -name '*.js' | head -1)"

expect_fail 4 'a page still claiming the site loads no analytics' \
  "sed -i 's|counts page views|loads no analytics|' \"\$D/evaluate/index.html\""

expect_fail 5 'an unevaluated MDX expression in a link' \
  "sed -i 's|</body>|<a href=\"%7Bhref(1)%7D\">x</a></body>|' \"\$D/about/index.html\""

expect_fail 6 'a known-real ABA routing number' \
  "sed -i 's|</body>|<p>091000019</p></body>|' \"\$D/docs/quickstart/index.html\""

expect_fail 7 'the mark in alt text outside <head>' \
  "sed -i 's|</body>|<img src=\"/og-image.png\" alt=\"$MARK_TEXT logo\"></body>|' \"\$D/about/index.html\""

expect_fail 7 'an og:image missing from the build' \
  "rm -f \"\$D/og-image.png\""

expect_fail 7 'a relative og:image' \
  "sed -i 's|property=\"og:image\" content=\"https://pacsmith.org|property=\"og:image\" content=\"|g' \"\$D/about/index.html\""

expect_fail 8 'the mark in the rendered PNG bytes' \
  "printf '$MARK_TEXT' >> \"\$D/og-image.png\""

expect_fail 8 'the mark in the source SVG' \
  "cp src/assets/og-image.svg \"\$WORK/bad.svg\" && sed -i 's|<title>|<title>$MARK_TEXT |' \"\$WORK/bad.svg\" && export OG_SVG=\"\$WORK/bad.svg\""

expect_fail 8 'a social preview of the wrong size' \
  "node scripts/make-test-png.mjs \"\$D/og-image.png\" 1200 400"

expect_fail 8 'a cropped social preview (flat bottom band)' \
  "node scripts/make-test-png.mjs \"\$D/og-image.png\" 1200 630"

expect_fail 9 'an edit link pointing at a file that does not exist' \
  "sed -i 's|edit/main/QUICKSTART.md|edit/main/QUICKSTART-renamed.md|' \"\$D/docs/quickstart/index.html\""

expect_fail 9 'every edit link removed from the build' \
  "find \"\$D\" -name '*.html' -exec sed -i 's|https://github.com/joaoabuenosi/fednow-oss/edit/main/|https://example.invalid/|g' {} +"

echo
if [ "$fail" -eq 0 ]; then
  echo "Self-test: $pass/$pass — every check caught its violation."
  exit 0
fi
echo "Self-test FAILED: $fail of $((pass + fail)) case(s) went undetected." >&2
echo "A check that cannot fail is not a check. Fix it before relying on this suite." >&2
exit 1
