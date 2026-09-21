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
 * The release asset table under "## Supply chain". Each row is
 * `| `<asset>` | <what it is> |`; the SBOM rows are the ones the site lists.
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

  return { all: rows, sboms, checksums: checksums.asset };
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
    },
    assets: {
      sboms: assets.sboms,
      sbomFormats: assets.sboms.map((s) => s.format),
      checksums: assets.checksums,
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
