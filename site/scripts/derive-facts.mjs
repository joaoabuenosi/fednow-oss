#!/usr/bin/env node
/**
 * derive-facts.mjs — run every SECURITY.md-and-friends extractor against a
 * checkout and print what the site would be allowed to state from it.
 *
 * Two uses:
 *
 *   node scripts/derive-facts.mjs            # what this checkout derives
 *   node scripts/derive-facts.mjs /tmp/root  # …from some other repository root
 *
 * The second form is what selftest-project-facts.sh drives: it builds a
 * throwaway root whose SECURITY.md has one fact reworded, runs this, and asserts
 * the specific error. Hence the contract below — on failure, print the error
 * message on stderr and nothing else, so the harness can match it exactly.
 *
 * Also worth having on its own: "what does the site think the release assets are
 * called?" is otherwise only answerable by building the site and reading
 * generated JSON.
 */

import { promises as fs } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { collectProjectFacts } from './project-facts.mjs';
import { extractVerifyCommands } from './sync-docs.mjs';

const SITE_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(process.argv[2] ?? path.join(SITE_DIR, '..'));

try {
  const facts = await collectProjectFacts(repoRoot);
  const security = await fs.readFile(path.join(repoRoot, 'SECURITY.md'), 'utf8');
  console.log(JSON.stringify({ ...facts, commands: extractVerifyCommands(security) }, null, 2));
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
