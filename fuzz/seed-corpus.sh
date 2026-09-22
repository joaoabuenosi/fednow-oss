#!/usr/bin/env bash
#
# Seed each fuzz target's corpus from the XML this repository already has.
#
# WHY DERIVE RATHER THAN COMMIT: libFuzzer starts from whatever is in the
# corpus directory. Starting from nothing means it spends its first minutes
# rediscovering that an ISO 20022 message begins with `<Document`, and it never
# reaches the validators at all — everything dies in the parser. Starting from a
# real message means the very first mutation is already past the XML frame and
# inside the fields the rules check.
#
# Those real messages are core/tests/fixtures/*.xml and
# conformance/vectors/*/*.xml, which are the project's source of truth for what
# a well-formed message looks like. Copying them into a committed corpus would
# make a second copy that stops matching the first the day someone edits one and
# not the other — the same reason the website generates its pages from the
# Markdown instead of duplicating it. So the corpus is generated and gitignored.
#
# SAFETY: every routing number in those files starts with 99, a prefix the ABA
# assigns to no institution (see the table in docs/handbook/README.md). This
# script asserts that before copying anything: a corpus is a pile of payment
# messages, and a real routing number in one would be a real bank's identifier
# sitting in CI artifacts and crash reports.
#
# Usage: ./fuzz/seed-corpus.sh   (from anywhere; paths are resolved from here)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"

# target name -> the globs whose matches are valid input for it.
seed() {
  local target="$1"; shift
  local dest="$here/corpus/$target"
  mkdir -p "$dest"

  local count=0
  local f
  for f in "$@"; do
    [ -f "$f" ] || continue

    # 99-prefix check. Anything else is a bug in the fixture, not in this
    # script, so fail loudly rather than skipping the file.
    local bad
    bad="$(grep -o '<MmbId>[0-9]\{9\}</MmbId>' "$f" \
      | sed 's/[^0-9]//g' \
      | grep -v '^99' || true)"
    if [ -n "$bad" ]; then
      echo "error: $f contains a routing number that does not start with 99:" >&2
      printf '  %s\n' $bad >&2
      echo "AGENTS.md forbids real routing numbers in this repository; fix the fixture." >&2
      exit 1
    fi

    cp "$f" "$dest/$(printf '%s' "${f#"$repo/"}" | tr '/' '_')"
    count=$((count + 1))
  done

  if [ "$count" -eq 0 ]; then
    echo "error: no seeds matched for ${target}; the fixture paths in this script have moved." >&2
    exit 1
  fi
  echo "  ${target}: ${count} seed(s) -> fuzz/corpus/${target}/"
}

echo "seeding fuzz corpora from fixtures and conformance vectors"

# Only inputs `pacs008::parse` is meant to accept: a bare pacs.008 Document.
# The envelope fixtures wrap one inside the FedNow technical envelope, so they
# are not valid input for this target and are left out on purpose.
seed pacs008_parse_validate \
  "$repo"/core/tests/fixtures/pacs008_valid*.xml \
  "$repo"/conformance/vectors/pacs008/*.xml

seed pacs002_parse_validate \
  "$repo"/core/tests/fixtures/pacs002_valid*.xml \
  "$repo"/conformance/vectors/pacs002/*.xml

echo "done."
