#!/usr/bin/env node
/**
 * project-facts.mjs — derive, at build time, every project fact the
 * hand-authored pages state, from the repository files that own them.
 *
 * WHY: the site used to hard-code "v0.3.1", "early development", which release
 * started signing, the SBOM formats and the cargo-audit / Scorecard /
 * Dependabot cadences. Every one of those is already stated somewhere in the
 * repository, and the two copies drift the first time someone bumps a version
 * or changes a cron and does not think about site/. A site that confidently
 * states last release's version to a bank's procurement team is worse than one
 * that states nothing.
 *
 * So the pages state none of them directly. This module parses the owning file
 * for each fact, and sync-docs.mjs writes the result to
 * src/generated/project-facts.json for the MDX pages to import.
 *
 * EVERY extractor throws on a shape it does not recognise. That is the point:
 * if a source file is reworded such that a fact can no longer be found, the
 * site build fails loudly instead of quietly publishing a stale value. A
 * thrown error here is the system working.
 */

import { promises as fs } from 'node:fs';
import path from 'node:path';

/** Throw with a message that says which file, which fact, and what to do. */
function fail(file, what, hint) {
  throw new Error(
    `project-facts: could not read ${what} from ${file}.\n` +
      `  ${hint}\n` +
      `  This is deliberate: the site derives that fact from ${file} rather than\n` +
      `  hard-coding it, so a reworded source must be re-taught here rather than\n` +
      `  silently publishing a stale value.`
  );
}

/** `[workspace.package] version` — the single source of truth for the version. */
function workspaceVersion(cargoToml) {
  const section = cargoToml.split(/^\[workspace\.package\]\s*$/m)[1];
  if (!section) fail('Cargo.toml', 'the version', 'No [workspace.package] section found.');
  const match = section.split(/^\[/m)[0].match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) fail('Cargo.toml', 'the version', 'No `version = "..."` key under [workspace.package].');
  return match[1];
}

/**
 * The project's own status line, which README.md states once:
 *   > ⚠️ **Early development (v0.3.1).** Not production-ready yet; …
 * Parsed rather than copied, and cross-checked against Cargo.toml below.
 */
function statusLine(readme) {
  const match = readme.match(/^>\s*[^\w\n]*\*\*([^(*]+?)\s*\((v[\d][^)]*)\)\.\*\*\s*(.+)$/m);
  if (!match) {
    fail(
      'README.md',
      'the project status line',
      'Expected a blockquote of the form `> ⚠️ **<status> (vX.Y.Z).** <rest>`.'
    );
  }
  const [, label, tag, rest] = match;
  return {
    label: label.trim(),
    tag: tag.trim(),
    productionReady: !/not production-ready/i.test(rest),
  };
}

/** Released versions, newest first, from the CHANGELOG's own headings. */
function releasedVersions(changelog) {
  const versions = [...changelog.matchAll(/^##\s*\[(\d+\.\d+\.\d+)\]/gm)].map((m) => m[1]);
  if (versions.length === 0) {
    fail('CHANGELOG.md', 'the released versions', 'No `## [X.Y.Z]` headings found.');
  }
  return versions;
}

/**
 * Strip markdown blockquote markers and collapse whitespace.
 *
 * Several of the facts below sit inside a `>` blockquote in SECURITY.md and
 * wrap across lines, so a naive regex misses them purely because of where the
 * line breaks fall. Normalising first means the extractor matches on the text,
 * not on the typography.
 */
const flatten = (markdown) => markdown.replace(/^\s*>\s?/gm, '').replace(/\s+/g, ' ');

/** "Signing starts with **v0.3.1**" — stated in SECURITY.md, not invented here. */
function signingStartsAt(security) {
  const match = flatten(security).match(/Signing starts with \*\*(v[\d.]+)\*\*/);
  if (!match) {
    fail('SECURITY.md', 'the first signed release', 'Expected the text `Signing starts with **vX.Y.Z**`.');
  }
  return match[1];
}

/** The release that shipped nothing, which /evaluate/ warns about by name. */
function releaseWithNoAssets(security) {
  const match = flatten(security).match(/(v[\d.]+) is a special case[\s\S]{0,400}?no assets at all/);
  if (!match) {
    fail(
      'SECURITY.md',
      'the release that carries no assets',
      'Expected text of the form `vX.Y.Z is a special case: … no assets at all`.'
    );
  }
  return match[1];
}

/**
 * Split an asset-table cell into its filename template and its extension.
 *
 * The cells are templates rather than literal filenames — `<asset>.sigstore.json`,
 * `fednow-oss-<tag>.intoto.jsonl` — because the real name depends on the tag and,
 * for signatures, on which asset is being signed. What the site needs to state is
 * the *extension*, since that is the part that identifies the format and the part
 * that changed when `.cosign.bundle` was retired.
 *
 * Takes everything from the first dot that follows the final `>` placeholder (or
 * the first dot at all, when the cell carries no placeholder), so a compound
 * extension such as `.sigstore.json` survives intact instead of being truncated
 * to `.json`.
 */
function assetExtension(asset, what) {
  const afterPlaceholder = asset.slice(asset.lastIndexOf('>') + 1);
  const dot = afterPlaceholder.indexOf('.');
  if (dot < 0) {
    fail('SECURITY.md', `the extension of ${what}`, `The asset cell \`${asset}\` has no file extension.`);
  }
  return afterPlaceholder.slice(dot);
}

/**
 * The release asset table under "## Supply chain". Each row is
 * `| `<asset>` | <what it is> |`.
 *
 * Four kinds of row are recognised, by what the description says rather than by
 * position: the SBOMs (CycloneDX / SPDX), the checksums (SHA-256), the Sigstore
 * signature bundle, and the SLSA build provenance. Every one of them is required.
 * A release that stopped shipping signatures, or a table reworded so they can no
 * longer be found, fails the build here rather than leaving /evaluate/ telling a
 * bank's procurement team to look for a file that is not there.
 */
function releaseAssets(security) {
  const section = security.split(/^## Supply chain\s*$/m)[1];
  if (!section) fail('SECURITY.md', 'the release assets', 'No `## Supply chain` section found.');

  const rows = [...section.split(/^##\s/m)[0].matchAll(/^\|\s*`([^`]+)`\s*\|\s*([^|]+?)\s*\|$/gm)].map(
    ([, asset, description]) => ({ asset, description })
  );
  if (rows.length === 0) {
    fail('SECURITY.md', 'the release assets', 'No `| `asset` | description |` rows under ## Supply chain.');
  }

  const sboms = rows
    .map((row) => {
      const format = row.description.match(/\b(CycloneDX|SPDX)\b/);
      return format ? { asset: row.asset, format: format[1], description: row.description } : null;
    })
    .filter(Boolean);
  if (sboms.length === 0) {
    fail('SECURITY.md', 'the SBOM assets', 'No release asset row mentions CycloneDX or SPDX.');
  }

  const checksums = rows.find((row) => /SHA-?256/i.test(row.description));
  if (!checksums) fail('SECURITY.md', 'the checksums asset', 'No release asset row mentions SHA-256.');

  // Matched on "signature" plus Sigstore, not on the extension: the extension is
  // the thing being derived, so keying the search off it would make this extractor
  // agree with itself rather than with SECURITY.md. That is exactly the failure
  // this table is meant to catch — the bundles were already Sigstore bundles while
  // being named `.cosign.bundle`.
  const signature = rows.find((row) => /\bsignature\b/i.test(row.description) && /Sigstore/i.test(row.description));
  if (!signature) {
    fail(
      'SECURITY.md',
      'the signature bundle asset',
      'No release asset row describes a Sigstore signature bundle. Expected a row whose description mentions both "Sigstore" and "signature".'
    );
  }

  const provenance = rows.find((row) => /\bprovenance\b/i.test(row.description));
  if (!provenance) {
    fail(
      'SECURITY.md',
      'the build provenance asset',
      'No release asset row describes build provenance. Expected a row whose description mentions "provenance".'
    );
  }

  return {
    all: rows,
    sboms,
    checksums: checksums.asset,
    signature: {
      asset: signature.asset,
      extension: assetExtension(signature.asset, 'the signature bundle'),
    },
    provenance: {
      asset: provenance.asset,
      extension: assetExtension(provenance.asset, 'the build provenance'),
    },
  };
}

/**
 * The signature extension a previous release used, and which release that was.
 *
 * /evaluate/ has to tell anyone verifying that release to substitute the old
 * name, and that is a fact SECURITY.md owns — stated in its own note under
 * "## Supply chain".
 *
 * Required, like every other extractor here, rather than optional. There will
 * come a day when that release falls out of the verification window and the
 * note is deleted; on that day this fails the build, and whoever deleted it also
 * deletes the paragraph on /evaluate/ that exists only to explain it. An
 * extractor that quietly returned null would instead let the page lose a
 * paragraph nobody noticed was gone.
 */
function legacySignatureName(security) {
  const match = flatten(security).match(/\*\*(v[\d.]+) named the same files `([^`]+)`/);
  if (!match) {
    fail(
      'SECURITY.md',
      'the previous signature bundle name',
      'Expected the text `**vX.Y.Z named the same files `.ext`**` under ## Supply chain. ' +
        'If that release no longer needs a note, delete the matching paragraph from ' +
        'site/src/content/docs/evaluate/index.mdx and this extractor together.'
    );
  }
  return { tag: match[1], extension: match[2] };
}

/**
 * "Build provenance starts with the first release after vX.Y.Z."
 *
 * The version pattern is `v\d+(\.\d+)+` rather than the looser `v[\d.]+` the
 * extractors above use: those all match a version followed by a space, while
 * this one sits at the end of a sentence, where `[\d.]+` happily swallows the
 * full stop and yields "v0.3.1.".
 */
function provenanceStartsAfter(security) {
  const match = flatten(security).match(/Build provenance starts with the first release after (v\d+(?:\.\d+)+)/);
  if (!match) {
    fail(
      'SECURITY.md',
      'the first release carrying build provenance',
      'Expected the text `Build provenance starts with the first release after vX.Y.Z`.'
    );
  }
  return match[1];
}

/**
 * Describe a 5-field cron in the words the site uses. Only the shapes this
 * repository's workflows actually use are recognised — a new shape should be
 * taught here rather than guessed at.
 */
function describeCron(cron, file) {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 5) fail(file, `the schedule "${cron}"`, 'Expected a 5-field cron expression.');
  const [, , dom, month, dow] = fields;
  if (dom === '*' && month === '*' && dow === '*') return 'daily';
  if (dom === '*' && month === '*' && /^[0-7]$/.test(dow)) return 'weekly';
  fail(file, `the schedule "${cron}"`, 'Only daily and weekly schedules are described; teach describeCron the new shape.');
}

/** A workflow's first `schedule: - cron:` entry, described in words. */
function workflowCadence(workflow, file) {
  const match = workflow.match(/^\s*schedule:\s*\n\s*-\s*cron:\s*["']([^"']+)["']/m);
  if (!match) fail(file, 'the schedule', 'No `schedule: - cron: "..."` trigger found.');
  return describeCron(match[1], file);
}

/** Does this workflow also run on pushes to a branch? */
function workflowPushBranches(workflow) {
  const match = workflow.match(/^\s*push:\s*\n\s*branches:\s*\[([^\]]+)\]/m);
  return match ? match[1].split(',').map((b) => b.trim().replace(/^["']|["']$/g, '')) : [];
}

/** Dependabot's cadence for a given ecosystem. */
function dependabotCadence(dependabot, ecosystem) {
  const block = dependabot.split(new RegExp(`package-ecosystem:\\s*${ecosystem}\\b`))[1];
  if (!block) fail('.github/dependabot.yml', `the ${ecosystem} schedule`, `No \`package-ecosystem: ${ecosystem}\` entry.`);
  const match = block.match(/^\s*interval:\s*(\w+)/m);
  if (!match) fail('.github/dependabot.yml', `the ${ecosystem} schedule`, 'No `interval:` under its `schedule:`.');
  return match[1];
}

/** Read every source and return the facts the site is allowed to state. */
export async function collectProjectFacts(repoRoot) {
  const read = async (rel) => {
    try {
      return await fs.readFile(path.join(repoRoot, rel), 'utf8');
    } catch {
      throw new Error(`project-facts: ${rel} is not readable from ${repoRoot}. The site build needs a full checkout.`);
    }
  };

  const [cargoToml, readme, changelog, security, auditWorkflow, scorecardWorkflow, dependabot] = await Promise.all([
    read('Cargo.toml'),
    read('README.md'),
    read('CHANGELOG.md'),
    read('SECURITY.md'),
    read('.github/workflows/audit.yml'),
    read('.github/workflows/scorecard.yml'),
    read('.github/dependabot.yml'),
  ]);

  const version = workspaceVersion(cargoToml);
  const status = statusLine(readme);

  // Cross-check: README's status line and Cargo.toml must agree. If a release
  // bumps one and forgets the other, the site says so rather than picking one.
  if (status.tag !== `v${version}`) {
    throw new Error(
      `project-facts: README.md's status line says ${status.tag} but Cargo.toml [workspace.package] says ${version}.\n` +
        '  These must agree — the site states one version, derived from both.'
    );
  }

  const released = releasedVersions(changelog);
  const assets = releaseAssets(security);

  return {
    note: 'Generated by site/scripts/sync-docs.mjs via project-facts.mjs — do not edit. Every value is parsed from the repository file that owns it.',
    version,
    tag: `v${version}`,
    status: {
      label: status.label,
      lowerLabel: status.label.charAt(0).toLowerCase() + status.label.slice(1),
      productionReady: status.productionReady,
    },
    releases: {
      all: released,
      latest: released[0],
      signingStartsAt: signingStartsAt(security),
      noAssets: releaseWithNoAssets(security),
      legacySignatureName: legacySignatureName(security),
      provenanceStartsAfter: provenanceStartsAfter(security),
    },
    assets: {
      sboms: assets.sboms,
      sbomFormats: assets.sboms.map((s) => s.format),
      checksums: assets.checksums,
      signature: assets.signature,
      provenance: assets.provenance,
    },
    cadence: {
      audit: workflowCadence(auditWorkflow, '.github/workflows/audit.yml'),
      scorecard: workflowCadence(scorecardWorkflow, '.github/workflows/scorecard.yml'),
      scorecardPushBranches: workflowPushBranches(scorecardWorkflow),
      dependabotCargo: dependabotCadence(dependabot, 'cargo'),
      dependabotActions: dependabotCadence(dependabot, 'github-actions'),
    },
  };
}
