#!/usr/bin/env bash
#
# Keeps SECURITY.md's "Practices" section true.
#
# SECURITY.md tells vendor-risk reviewers two things about every workflow in
# this repository:
#
#   - Third-party GitHub Actions are pinned to a commit SHA, not a tag.
#   - Workflows declare `permissions: {}` at the top level; each job requests
#     only the scopes it needs.
#
# Those sentences were true of release.yml and scorecard.yml and false of the
# other three until #84. A document that is true on the day it is written and
# false a month later is worse than no document, so this script re-checks both
# claims on every CI run and fails the build when a workflow drifts.
#
# Deliberately plain: bash, grep and sed only. No third-party action, no
# network, no YAML parser to install — the whole point is that the thing
# guarding the supply chain does not itself add supply-chain surface. Runs in
# well under a second.
#
# Rules, per workflow file in .github/workflows/:
#
#   1. Every `uses:` must reference a full 40-hex commit SHA. Exempt: local
#      actions and reusable workflows (`./...`), which are this repository's
#      own code and move with the commit under test, and `docker://` refs,
#      which carry their own digest or tag semantics.
#   2. Every SHA pin must carry a trailing `# <version>` comment. It is what
#      makes a pin readable, and Dependabot reads and rewrites that comment
#      when it bumps the SHA (.github/dependabot.yml) — drop it and the pin
#      silently stops being updated.
#   3. Every workflow must declare a top-level `permissions:` key, so no job
#      ever inherits the repository default GITHUB_TOKEN scope by accident.
#
# What this script deliberately does NOT do: re-resolve the version comment
# against upstream. Tags such as `dtolnay/rust-toolchain`'s `v1` and
# `rustsec/audit-check`'s `v2` are *moved* by their maintainers as new
# versions ship, so "does the SHA still match the tag?" fails for pins that
# are perfectly fine, and passes for a pin whose action.yml no longer accepts
# the inputs we hand it (the bug that broke v0.3.0). The check that catches
# both is reading the pinned commit's action.yml at review time; that is a
# human step, recorded in the PR that changes a pin.

set -euo pipefail

workflow_dir="${1:-.github/workflows}"
failures=0

fail() {
  # ::error:: renders inline on the run's Summary page and, with file/line,
  # as an annotation on the offending line in the diff.
  printf '::error file=%s,line=%s::%s\n' "$1" "$2" "$3"
  failures=$((failures + 1))
}

shopt -s nullglob
workflows=("$workflow_dir"/*.yml "$workflow_dir"/*.yaml)
shopt -u nullglob

if [ ${#workflows[@]} -eq 0 ]; then
  echo "::error::no workflow files found under ${workflow_dir} — has the path moved?"
  exit 1
fi

for wf in "${workflows[@]}"; do
  # --- Rule 3: top-level permissions -----------------------------------
  # A top-level key sits in column 0; a job-level `permissions:` is indented.
  if grep -qE '^permissions:' "$wf"; then
    printf '  ok   %-44s top-level permissions declared\n' "$wf"
  else
    fail "$wf" 1 "${wf} declares no top-level 'permissions:' key, so every job in it inherits the repository default GITHUB_TOKEN scope. Add 'permissions: {}' at the top level and give each job only the scopes it needs."
  fi

  # --- Rules 1 and 2: pinned uses: --------------------------------------
  while IFS= read -r hit; do
    line="${hit%%:*}"
    text="${hit#*:}"

    # Everything after `uses:`, minus surrounding whitespace and quotes.
    value="$(printf '%s\n' "$text" \
      | sed -E 's/^[[:space:]]*(-[[:space:]]+)?uses:[[:space:]]*//')"
    comment=""
    case "$value" in
      *'#'*) comment="${value#*#}" ;;
    esac
    ref_spec="$(printf '%s\n' "$value" | sed -E 's/[[:space:]]*#.*$//; s/^["'"'"']//; s/["'"'"']$//; s/[[:space:]]+$//')"

    case "$ref_spec" in
      ./*)
        printf '  ok   %-44s %-4s local action/workflow (%s)\n' "$wf" "L$line" "$ref_spec"
        continue
        ;;
      docker://*)
        printf '  ok   %-44s %-4s docker ref (%s)\n' "$wf" "L$line" "$ref_spec"
        continue
        ;;
    esac

    ref="${ref_spec##*@}"
    if [ "$ref" = "$ref_spec" ]; then
      fail "$wf" "$line" "${ref_spec} has no '@<ref>' at all. Third-party actions must be pinned to a full 40-character commit SHA."
      continue
    fi

    if ! printf '%s' "$ref" | grep -qE '^[0-9a-fA-F]{40}$'; then
      fail "$wf" "$line" "${ref_spec} is pinned to '${ref}', which is not a 40-character commit SHA. Tags and branches can be re-pointed by the action's owner at any time; resolve it with 'git ls-remote https://github.com/${ref_spec%@*} refs/tags/<version>' (dereference annotated tags with '^{}') and pin the SHA with a trailing '# <version>' comment."
      continue
    fi

    if [ -z "$(printf '%s' "$comment" | tr -d '[:space:]')" ]; then
      fail "$wf" "$line" "${ref_spec%@*} is SHA-pinned but carries no trailing '# <version>' comment. Add one: Dependabot reads it to know which version the SHA stands for, and rewrites it when it bumps the pin."
      continue
    fi

    printf '  ok   %-44s %-4s %s #%s\n' "$wf" "L$line" "${ref_spec%@*}" "$comment"
  done < <(grep -nE '^[[:space:]]*(-[[:space:]]+)?uses:[[:space:]]' "$wf" || true)
done

echo
if [ "$failures" -ne 0 ]; then
  echo "workflow hardening: ${failures} problem(s) found — see the annotations above."
  echo "SECURITY.md's 'Practices' section promises both of these to anyone auditing this project."
  exit 1
fi

echo "workflow hardening: all ${#workflows[@]} workflow(s) pass."
