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
 *     verification command that does not work.
 *  4. Asserts the licence is still Apache-2.0, because /evaluate/licence/ says
 *     so in prose.
 */

import { promises as fs } from 'node:fs';
import { existsSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { REPO_URL, href } from '../site.config.mjs';

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
 * Pull the exact verification command out of SECURITY.md so /evaluate/ can show
 * it without keeping a second copy that could rot.
 */
function extractVerifyCommand(security) {
  const section = security.split(/^### Verifying a release\s*$/m)[1];
  if (!section) throw new Error('SECURITY.md: "### Verifying a release" section not found.');

  const blocks = [...section.matchAll(/```console\n([\s\S]*?)```/g)].map((m) => m[1]);
  const block = blocks.find((b) => b.includes('cosign verify-blob'));
  if (!block) throw new Error('SECURITY.md: no ```console block containing `cosign verify-blob` under "Verifying a release".');

  // Keep the `$`-prefixed input lines; drop the sample output (`Verified OK`).
  const command = block
    .split('\n')
    .filter((line) => line.startsWith('$') || /^\s{2,}/.test(line))
    .join('\n')
    .trimEnd();

  if (!command.includes('--certificate-identity') || !command.includes('--certificate-oidc-issuer')) {
    throw new Error('SECURITY.md: the extracted cosign command is missing --certificate-identity/--certificate-oidc-issuer.');
  }
  return command;
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
        command: extractVerifyCommand(security),
      },
      null,
      2
    ) + '\n'
  );

  const licence = await fs.readFile(path.join(REPO_ROOT, 'LICENSE'), 'utf8');
  if (!licence.includes('Apache License') || !licence.includes('Version 2.0')) {
    throw new Error('LICENSE is no longer Apache-2.0, but /evaluate/licence/ says it is. Update the page.');
  }

  console.log(`sync-docs: ${PAGES.length} pages generated from the repository markdown.`);
}

main().catch((error) => {
  console.error('sync-docs failed:', error.message);
  process.exit(1);
});
