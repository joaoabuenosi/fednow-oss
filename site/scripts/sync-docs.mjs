#!/usr/bin/env node
/**
 * sync-docs.mjs — pull the repository's markdown into the Starlight content
 * collection at build time.
 *
 * WHY: README.md, QUICKSTART.md, docs/handbook/** and SECURITY.md are the
 * source of truth and stay that way. Copying them into site/ by hand would
 * guarantee drift the first time someone edits one and not the other. This
 * script regenerates the site's pages from those files on every `npm run
 * build` and `npm run dev`; the generated files are gitignored.
 *
 * WHAT IT DOES BEYOND COPYING:
 *  1. Replaces each source H1 with an explicit, TRADEMARK-SAFE frontmatter
 *     title. Source H1s contain the FedNow mark; Starlight would put the title
 *     straight into <title> and the Open Graph tags, which the mark's terms
 *     forbid. The prose underneath keeps the mark where it belongs: visible
 *     body text, used descriptively.
 *  2. Rewrites relative markdown links: to a site URL when the target is also
 *     published here, otherwise to the file on GitHub. A dead relative link in
 *     the rendered site is a broken promise; both cases are resolved here.
 *  3. Extracts the exact `cosign verify-blob` command from SECURITY.md into
 *     src/generated/verify-release.json, so the command shown on /evaluate/
 *     cannot drift from the one the security policy documents. If the section
 *     or the command moves, this script fails the build rather than shipping a
 *     verification command that does not work. The `gh attestation verify`
 *     command for the SLSA build provenance comes out of the same file by the
 *     same rules, so /evaluate/ names no signature or provenance file itself.
 *  4. Asserts the licence is still Apache-2.0, because /evaluate/licence/ says
 *     so in prose.
 *  5. Derives every project fact the hand-authored pages state — the version,
 *     the release status, which release started signing, the SBOM formats, and
 *     the cargo-audit / Scorecard / Dependabot cadences — from the repository
 *     files that own them, into src/generated/project-facts.json. The pages
 *     import it rather than hard-coding values that go stale the first time
 *     someone bumps a version and does not think about site/. See
 *     project-facts.mjs; a source it can no longer parse fails the build.
 */

import { promises as fs } from 'node:fs';
import { existsSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { REPO_URL, href } from '../site.config.mjs';
import { collectProjectFacts } from './project-facts.mjs';

const SITE_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REPO_ROOT = path.resolve(SITE_DIR, '..');
const DOCS_DIR = path.join(SITE_DIR, 'src', 'content', 'docs');
const GENERATED_DIR = path.join(SITE_DIR, 'src', 'generated');

const BLOB = `${REPO_URL}/blob/main/`;
const TREE = `${REPO_URL}/tree/main/`;

/**
 * The published set. `src` is repo-relative; `out` is relative to
 * src/content/docs. `title` and `description` are written into frontmatter and
 * therefore end up in <title> and <meta name="description"> — they must not
 * contain the FedNow mark.
 */
const PAGES = [
  {
    src: 'README.md',
    out: 'docs/overview.md',
    title: 'Overview',
    description: 'What Pacsmith is, which components ship today, and where the project stands.',
  },
  {
    src: 'QUICKSTART.md',
    out: 'docs/quickstart.md',
    title: 'Quick start',
    description: 'Send your first instant payment in five minutes with Docker and curl — no Rust required.',
  },
  {
    src: 'docs/handbook/README.md',
    out: 'docs/handbook/index.md',
    title: 'Integration handbook',
    description: 'Production-oriented guidance for building instant-payment send capability, chapter by chapter.',
  },
  {
    src: 'docs/handbook/01-credit-transfer-flow.md',
    out: 'docs/handbook/credit-transfer-flow.md',
    title: 'Chapter 1 — The credit transfer flow',
    description: 'pacs.008 out, pacs.002 back: the exchange every other flow is a variation of.',
  },
  {
    src: 'docs/handbook/02-timeout-reconciliation.md',
    out: 'docs/handbook/timeout-reconciliation.md',
    title: 'Chapter 2 — Timeout reconciliation',
    description: 'You sent a payment and nothing came back. The hard case, and why a resend is never the answer.',
  },
  {
    src: 'docs/handbook/04-zero-to-ctp.md',
    out: 'docs/handbook/zero-to-ctp.md',
    title: 'Chapter 4 — From zero to certification testing',
    description: 'Mapping the required test scenarios family by family, and rehearsing them locally first.',
  },
  {
    src: 'docs/handbook/05-returns.md',
    out: 'docs/handbook/returns.md',
    title: 'Chapter 5 — Returns',
    description: 'Settlement is final. camt.056, camt.029 and pacs.004 are how settled money comes back.',
  },
  {
    src: 'gateway/README.md',
    out: 'docs/gateway-api.md',
    title: 'Gateway API',
    description: 'The gateway REST API: idempotency-keyed submits, payment state, reconciliation and ops endpoints.',
  },
  {
    src: 'sdk/python/README.md',
    out: 'docs/sdks/python.md',
    title: 'Python SDK',
    description: 'A zero-dependency Python client for the gateway REST API.',
  },
  {
    src: 'sdk/java/README.md',
    out: 'docs/sdks/java.md',
    title: 'Java SDK',
    description: 'A Java 17+ client for the gateway REST API, with one runtime dependency.',
  },
  {
    src: 'simulator/README.md',
    out: 'docs/simulator.md',
    title: 'Simulator',
    description: 'A local service simulator with configurable accept, reject, pending and timeout scenarios.',
  },
  {
    src: 'conformance/README.md',
    out: 'docs/conformance.md',
    title: 'Conformance suite',
    description: 'A language-agnostic vector corpus, a validator CLI and a live scenario runner.',
  },
  {
    src: 'SECURITY.md',
    out: 'evaluate/security-policy.md',
    title: 'Security policy',
    description:
      'Vulnerability disclosure, response times, and the supply chain: keyless signing, SBOMs and release verification.',
  },
];

/** Repo-relative source path -> site path, for rewriting links between published pages. */
const pageLinks = new Map(
  PAGES.map(({ src, out }) => [src, '/' + out.replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '') + '/'])
);

/** Directory links (`gateway/`, `sdk/python/`, …) land on that component's page. */
const dirLinks = new Map([
  ['gateway', pageLinks.get('gateway/README.md')],
  ['simulator', pageLinks.get('simulator/README.md')],
  ['conformance', pageLinks.get('conformance/README.md')],
  ['sdk/python', pageLinks.get('sdk/python/README.md')],
  ['sdk/java', pageLinks.get('sdk/java/README.md')],
  ['docs/handbook', pageLinks.get('docs/handbook/README.md')],
]);

const FORBIDDEN_IN_METADATA = /fednow/i;

function assertMarkFree(where, value) {
  if (FORBIDDEN_IN_METADATA.test(value)) {
    throw new Error(
      `Trademark rule: "${where}" becomes page metadata (<title>/<meta>/Open Graph) and must not ` +
        `contain the FedNow mark. Offending value: ${JSON.stringify(value)}`
    );
  }
}

/** YAML-quote a scalar for frontmatter. */
const yaml = (s) => `"${s.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;

/**
 * Resolve one relative markdown link target against the file that contains it.
 * Returns the replacement href, or null to leave the link untouched.
 */
function resolveLink(target, fromSrc) {
  if (/^(https?:|mailto:|#|\/\/)/i.test(target)) return null;

  const [rawPath, ...hashParts] = target.split('#');
  const hash = hashParts.length ? '#' + hashParts.join('#') : '';
  if (!rawPath) return null;

  const trailingSlash = rawPath.endsWith('/');

  // A leading "/" is resolved from the REPOSITORY ROOT, not from the directory
  // of the file containing the link. Joining it onto the containing directory
  // instead — which is what a naive join does — either throws with a nonsense
  // path or, worse, silently resolves to a real-but-wrong file when the nested
  // path happens to exist. Neither is acceptable in generated output, so the
  // case is handled explicitly; anything that still resolves nowhere throws
  // below, as every unresolvable link here does.
  const rootRelative = rawPath.startsWith('/');
  const resolved = path
    .posix
    .normalize(rootRelative ? rawPath.slice(1) : path.posix.join(path.posix.dirname(fromSrc), rawPath))
    .replace(/^\.\//, '')
    .replace(/\/$/, '');

  if (resolved.startsWith('..')) return null; // outside the repository — leave alone
  if (resolved === '' || resolved === '.') return TREE + hash; // a link to the repository root

  if (pageLinks.has(resolved)) return href(pageLinks.get(resolved)) + hash;
  if (dirLinks.has(resolved) && dirLinks.get(resolved)) return href(dirLinks.get(resolved)) + hash;

  const onDisk = path.join(REPO_ROOT, resolved);
  if (!existsSync(onDisk)) {
    throw new Error(`Broken relative link in ${fromSrc}: "${target}" resolves to ${resolved}, which does not exist.`);
  }
  const isDir = statSync(onDisk).isDirectory();
  return (isDir || trailingSlash ? TREE : BLOB) + resolved + hash;
}

/** Rewrite every inline markdown link target in `body`. */
function rewriteLinks(body, fromSrc) {
  // Inline links only: [text](target) and [text](target "title").
  return body.replace(/\]\(([^)\s]+)(\s+"[^"]*")?\)/g, (match, target, title = '') => {
    const replacement = resolveLink(target, fromSrc);
    return replacement === null ? match : `](${replacement}${title})`;
  });
}

/** Drop the leading H1 — the frontmatter title replaces it. */
function stripLeadingH1(body, src) {
  const match = body.match(/^\s*#\s+.*\n/);
  if (!match) throw new Error(`${src} has no leading H1; the sync script expects one.`);
  return body.slice(match[0].length).replace(/^\s*\n/, '');
}

/**
 * Find the first ```console block under `heading` whose command is `invocation`,
 * and return just the typed input: the `$`-prefixed lines plus their indented
 * continuations, with the sample output (`Verified OK`, a digest line) dropped.
 *
 * Shared by both extractors below so the two commands on /evaluate/ are pulled
 * out of SECURITY.md by identical rules — and so a change to what counts as
 * "the command" cannot apply to one of them and not the other.
 */
function extractConsoleCommand(security, heading, invocation, requiredFlags) {
  const section = security.split(new RegExp(`^### ${heading}\\s*$`, 'm'))[1];
  if (!section) throw new Error(`SECURITY.md: "### ${heading}" section not found.`);

  const blocks = [...section.matchAll(/```console\n([\s\S]*?)```/g)].map((m) => m[1]);
  const block = blocks.find((b) => b.includes(invocation));
  if (!block) {
    throw new Error(`SECURITY.md: no \`\`\`console block containing \`${invocation}\` under "${heading}".`);
  }

  const command = block
    .split('\n')
    .filter((line) => line.startsWith('$') || /^\s{2,}/.test(line))
    .join('\n')
    .trimEnd();

  // The flags that carry the security property. Without them the command still
  // runs and still prints success, against an identity nobody pinned — which is
  // the one failure mode a copy-pasteable verification command must not have.
  const missing = requiredFlags.filter((flag) => !command.includes(flag));
  if (missing.length > 0) {
    throw new Error(
      `SECURITY.md: the extracted \`${invocation}\` command is missing ${missing.join(', ')}.`
    );
  }
  return command;
}

/**
 * Pull the exact verification commands out of SECURITY.md so /evaluate/ can show
 * them without keeping a second copy that could rot.
 *
 * Two of them, because a release carries two independent claims and they are
 * checked with different tools: the cosign signature over the bytes, and the
 * SLSA build provenance over what produced them. Deriving both means /evaluate/
 * never names a signature or provenance file itself — if the bundles are
 * renamed again, as `.cosign.bundle` was, the page follows automatically.
 */
export function extractVerifyCommands(security) {
  return {
    command: extractConsoleCommand(security, 'Verifying a release', 'cosign verify-blob', [
      '--certificate-identity',
      '--certificate-oidc-issuer',
    ]),
    provenanceCommand: extractConsoleCommand(security, 'Verifying build provenance', 'gh attestation verify', [
      '--repo',
      '--signer-workflow',
    ]),
  };
}

/**
 * The sync reads files from the repository root, one level above site/. On
 * Vercel the project's Root Directory is `site`, and by default Vercel uploads
 * ONLY that directory — which would silently produce a site with no docs. Fail
 * loudly instead, naming the setting that fixes it.
 */
function assertRepoRootIsAvailable() {
  const missing = [...new Set(PAGES.map((p) => p.src)), 'LICENSE'].filter(
    (rel) => !existsSync(path.join(REPO_ROOT, rel))
  );
  if (missing.length === 0) return;

  throw new Error(
    [
      `the repository root is not readable from ${SITE_DIR}.`,
      `Missing: ${missing.join(', ')}`,
      '',
      'This site is generated from the repository\'s own markdown on purpose, so the',
      'docs cannot drift. That means the build needs the files above site/.',
      '',
      'On Vercel: Project Settings -> Build and Deployment -> Root Directory,',
      'enable "Include files outside the root directory in the build step".',
      'Locally: run the build from a full checkout, not from a copy of site/ alone.',
    ].join('\n')
  );
}

async function main() {
  assertRepoRootIsAvailable();

  await fs.rm(path.join(DOCS_DIR, 'docs'), { recursive: true, force: true });
  await fs.rm(path.join(DOCS_DIR, 'evaluate', 'security-policy.md'), { force: true });
  await fs.rm(GENERATED_DIR, { recursive: true, force: true });

  for (const page of PAGES) {
    assertMarkFree(`${page.out} title`, page.title);
    assertMarkFree(`${page.out} description`, page.description);

    const abs = path.join(REPO_ROOT, page.src);
    const raw = await fs.readFile(abs, 'utf8');
    const body = rewriteLinks(stripLeadingH1(raw, page.src), page.src);

    const frontmatter = [
      '---',
      `title: ${yaml(page.title)}`,
      `description: ${yaml(page.description)}`,
      'editUrl: ' + yaml(BLOB.replace('/blob/', '/edit/') + page.src),
      '---',
      '',
      `<!-- Generated by site/scripts/sync-docs.mjs from ${page.src}. Edit that file, not this one. -->`,
      '',
    ].join('\n');

    const dest = path.join(DOCS_DIR, page.out);
    await fs.mkdir(path.dirname(dest), { recursive: true });
    await fs.writeFile(dest, frontmatter + body);
  }

  const security = await fs.readFile(path.join(REPO_ROOT, 'SECURITY.md'), 'utf8');
  await fs.mkdir(GENERATED_DIR, { recursive: true });
  await fs.writeFile(
    path.join(GENERATED_DIR, 'verify-release.json'),
    JSON.stringify(
      {
        note: 'Generated by site/scripts/sync-docs.mjs from SECURITY.md — do not edit.',
        ...extractVerifyCommands(security),
      },
      null,
      2
    ) + '\n'
  );

  const facts = await collectProjectFacts(REPO_ROOT);
  await fs.writeFile(
    path.join(GENERATED_DIR, 'project-facts.json'),
    JSON.stringify(facts, null, 2) + '\n'
  );

  const licence = await fs.readFile(path.join(REPO_ROOT, 'LICENSE'), 'utf8');
  if (!licence.includes('Apache License') || !licence.includes('Version 2.0')) {
    throw new Error('LICENSE is no longer Apache-2.0, but /evaluate/licence/ says it is. Update the page.');
  }

  console.log(
    `sync-docs: ${PAGES.length} pages generated from the repository markdown; ` +
      `project facts derived for ${facts.tag} (${facts.status.lowerLabel}).`
  );
}

// Run the sync only when this file is the process entry point. It is also
// imported as a module — scripts/derive-facts.mjs pulls `extractVerifyCommands`
// out of here so selftest-project-facts.sh can prove those extractors reject a
// reworded SECURITY.md. Without this guard, importing the module would run the
// whole sync and write into src/generated/ as a side effect of asking a question.
const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedDirectly) {
  main().catch((error) => {
    console.error('sync-docs failed:', error.message);
    process.exit(1);
  });
}
