#!/usr/bin/env bash
#
# selftest-project-facts.sh — prove that every extractor behind the site's
# derived facts actually refuses a SECURITY.md it no longer understands.
#
# WHY THIS EXISTS. project-facts.mjs and sync-docs.mjs exist so that no page
# names a release asset, a version or a verification flag itself: each is parsed
# at build time out of the repository file that owns it. That only protects
# anything if a source the extractor can no longer parse *fails the build*. An
# extractor that quietly returns undefined publishes a page reading
# "verify with " and nobody notices.
#
# The rename this machinery was written for is the cautionary tale: release
# bundles were correct Sigstore bundles named `.cosign.bundle` for a whole
# release, and every tool that looks for signatures by filename — including
# OpenSSF Scorecard — reported the project as shipping none. A check that cannot
# fail reads as a guarantee, which is worse than no check at all.
#
# So each case below takes a real checkout, rewords ONE fact in a throwaway copy
# of SECURITY.md, and asserts the extractor fails WITH THE SPECIFIC MESSAGE for
# that fact. Asserting a non-zero exit is not enough: every one of these
# rewordings would also "fail" if project-facts.mjs had a syntax error, and the
# suite would look green while testing nothing.
#
# Run by `npm run check`, alongside selftest-checks.sh.
set -uo pipefail

SITE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SITE_DIR"
REPO_ROOT="$(cd "$SITE_DIR/.." && pwd)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

pass=0
fail=0

# A throwaway repository root: every top-level entry symlinked back to the real
# checkout, except SECURITY.md, which is a real copy for the mutation to edit.
#
# Symlinks rather than a copy so this keeps working when an extractor starts
# reading a source nobody listed here — the alternative, enumerating the seven
# files project-facts.mjs happens to read today, is a second thing to keep in
# step with it.
fixture_root() {
  local root="$WORK/root"
  rm -rf "$root"
  mkdir -p "$root"
  local entry
  for entry in "$REPO_ROOT"/*; do
    ln -s "$entry" "$root/$(basename "$entry")"
  done
  ln -s "$REPO_ROOT/.github" "$root/.github"
  rm -f "$root/SECURITY.md"
  cp "$REPO_ROOT/SECURITY.md" "$root/SECURITY.md"
  printf '%s' "$root"
}

# expect_fail <description> <mutation> <required message fragment>...
#
# The mutation is shell run with $S bound to the throwaway SECURITY.md. Every
# fragment must appear in the extractor's output, which is flattened to one line
# first so a fragment may span the message's own line breaks.
expect_fail() {
  local description="$1" mutation="$2"; shift 2
  local root; root="$(fixture_root)"
  local S="$root/SECURITY.md"
  local out="$WORK/out.txt"

  if ! eval "$mutation" >/dev/null 2>&1; then
    printf '  BROKEN %s\n' "$description"
    printf '         the mutation itself failed to apply — the test is wrong, not the extractor.\n'
    fail=$((fail + 1))
    return
  fi
  if cmp -s "$S" "$REPO_ROOT/SECURITY.md"; then
    printf '  BROKEN %s\n' "$description"
    printf '         the mutation changed nothing — it matched no text, so this case proves nothing.\n'
    fail=$((fail + 1))
    return
  fi

  node scripts/derive-facts.mjs "$root" >"$out" 2>&1
  local code=$? flat missing=()
  flat="$(tr '\n' ' ' <"$out")"

  local fragment
  for fragment in "$@"; do
    case "$flat" in *"$fragment"*) ;; *) missing+=("$fragment") ;; esac
  done

  if [ "$code" -ne 0 ] && [ "${#missing[@]}" -eq 0 ]; then
    printf '  ok    %s\n' "$description"
    printf '        -> %s\n' "$(head -1 "$out")"
    pass=$((pass + 1))
  else
    printf '  BROKEN %s\n' "$description"
    if [ "$code" -eq 0 ]; then
      printf '         the extractor SUCCEEDED on a source it should not understand.\n'
    else
      printf '         it failed, but not for this reason. Missing from the message: %s\n' "${missing[*]}"
    fi
    sed 's/^/           /' "$out"
    fail=$((fail + 1))
  fi
}

echo "Self-testing the derived-facts extractors: each reworded fact below MUST be refused."
echo

# Positive control, and not merely "exit 0": if the extractors stopped finding
# the assets at all, every expected failure below would still "pass" for the
# wrong reason. So assert the values this checkout must actually derive.
baseline="$WORK/baseline.json"
if node scripts/derive-facts.mjs "$(fixture_root)" >"$baseline" 2>&1 &&
  grep -q '"\.sigstore\.json"' "$baseline" &&
  grep -q '"\.intoto\.jsonl"' "$baseline" &&
  grep -q '"\.cosign\.bundle"' "$baseline" &&
  grep -q 'signer-workflow' "$baseline"; then
  echo "  ok    baseline  the unmodified SECURITY.md yields .sigstore.json, .intoto.jsonl, .cosign.bundle and both commands"
  pass=$((pass + 1))
else
  echo "  BROKEN baseline the unmodified SECURITY.md does not derive the expected facts —"
  echo "         the cases below prove nothing. Output:"
  sed 's/^/           /' "$baseline"
  fail=$((fail + 1))
fi
echo

# --- the asset table: a fact that can no longer be found ------------------

expect_fail 'the whole "## Supply chain" section renamed' \
  "sed -i 's|^## Supply chain\$|## Release artifacts|' \"\$S\"" \
  'could not read the release assets' '## Supply chain'

expect_fail 'the signature row reworded past recognition' \
  "sed -i 's|signature bundle per asset above|opaque blob per asset above|' \"\$S\"" \
  'could not read the signature bundle asset' 'No release asset row matched'

expect_fail 'the provenance row reworded past recognition' \
  "sed -i 's|build provenance covering all four assets|attached metadata covering all four assets|' \"\$S\"" \
  'could not read the build provenance asset' 'No release asset row matched'

expect_fail 'the signature asset cell loses its extension' \
  "sed -i 's|\`<asset>\\.sigstore\\.json\`|\`<asset>\`|' \"\$S\"" \
  'could not read the extension of the signature bundle'

# --- the asset table: a fact that can now be found twice -------------------
#
# The 🟢 of classifying by `.find()` was that it never noticed a second match.
# These three are the regression tests for that.

expect_fail 'a second row also describing a Sigstore signature bundle' \
  "sed -i '/\`<asset>\\.sigstore\\.json\`/a | \`<asset>.sig\` | a detached Sigstore signature per asset above |' \"\$S\"" \
  'could not read the signature bundle asset' 'rows matched, so which one the site should name is ambiguous' \
  '`<asset>.sigstore.json`, `<asset>.sig`'

expect_fail 'a second row also describing build provenance' \
  "sed -i '/\`fednow-oss-<tag>\\.intoto\\.jsonl\`/a | \`fednow-oss-<tag>.slsa.json\` | older build provenance, kept for one release |' \"\$S\"" \
  'could not read the build provenance asset' 'rows matched, so which one the site should name is ambiguous'

expect_fail 'a second row also describing SHA-256 checksums' \
  "sed -i '/^| \`SHA256SUMS\` |/a | \`SHA256SUMS.txt\` | SHA-256 of the same set, as plain text |' \"\$S\"" \
  'could not read the checksums asset' 'rows matched, so which one the site should name is ambiguous'

# --- the prose facts -------------------------------------------------------

expect_fail 'the previous-bundle-name note deleted' \
  "sed -i 's|\*\*v0\\.3\\.1 named the same files \`\\.cosign\\.bundle\`\\.\*\*|A note about an earlier release.|' \"\$S\"" \
  'could not read the previous signature bundle name'

expect_fail 'the "Build provenance starts with" sentence reworded' \
  "sed -i 's|Build provenance starts with the first release after v0\\.3\\.1\\.|Provenance came along later.|' \"\$S\"" \
  'could not read the first release carrying build provenance'

expect_fail 'the "Signing starts with" sentence reworded' \
  "sed -i 's|Signing starts with \*\*v0\\.3\\.1\*\*|Signing began at v0.3.1|' \"\$S\"" \
  'could not read the first signed release'

# --- the verification commands --------------------------------------------

expect_fail 'the "### Verifying build provenance" heading renamed' \
  "sed -i 's|^### Verifying build provenance\$|### Checking provenance|' \"\$S\"" \
  '"### Verifying build provenance" section not found'

expect_fail 'the gh command loses --signer-workflow' \
  "sed -i '0,/^    --signer-workflow /{/^    --signer-workflow /d}' \"\$S\"" \
  'gh attestation verify` command is missing --signer-workflow'

expect_fail 'the cosign command loses --certificate-identity' \
  "sed -i '0,/^    --certificate-identity \"/{/^    --certificate-identity \"/d}' \"\$S\"" \
  'cosign verify-blob` command is missing --certificate-identity'

echo
if [ "$fail" -eq 0 ]; then
  echo "Self-test: $pass/$pass — every reworded fact was refused, with its own message."
  exit 0
fi
echo "Self-test FAILED: $fail of $((pass + fail)) case(s) were not refused as expected." >&2
echo "An extractor that cannot fail is not deriving a fact, it is publishing a guess." >&2
exit 1
