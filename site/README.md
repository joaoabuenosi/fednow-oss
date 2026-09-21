# Pacsmith website

The project site, built with [Astro Starlight](https://starlight.astro.build/).

```sh
npm ci
npm run dev      # sync + dev server
npm run build    # sync + static build into dist/
npm run check    # dependency audit + trademark and footer checks over dist/
```

Node 22+ (`.nvmrc`). **Run it from a full checkout** — the build reads markdown from the
repository root, one level up.

## The docs are not stored here

`src/content/docs/docs/` and `src/content/docs/evaluate/security-policy.md` are
**generated** and gitignored. `scripts/sync-docs.mjs` regenerates them from the
repository's own markdown on every `dev` and `build`:

| Source (the truth) | Published as |
|---|---|
| `README.md` | `/docs/overview/` |
| `QUICKSTART.md` | `/docs/quickstart/` |
| `docs/handbook/*.md` | `/docs/handbook/…` |
| `gateway/README.md` | `/docs/gateway-api/` |
| `sdk/python/README.md`, `sdk/java/README.md` | `/docs/sdks/…` |
| `simulator/README.md`, `conformance/README.md` | `/docs/simulator/`, `/docs/conformance/` |
| `SECURITY.md` | `/evaluate/security-policy/` |

Edit the source file, never the generated page. The sync also:

- replaces each source H1 with a trademark-safe frontmatter title (see below);
- rewrites relative links — to a site URL if the target is published here, to GitHub
  otherwise — and **fails the build** on a relative link that resolves nowhere;
- extracts the exact `cosign verify-blob` command from `SECURITY.md` into
  `src/generated/verify-release.json`, so `/evaluate/` cannot show a stale command;
- asserts `LICENSE` is still Apache-2.0, because `/evaluate/licence/` says so in prose.

Hand-authored pages: `index.mdx` (home), `evaluate/index.mdx`, `evaluate/licence.md`,
`about.md`.

## Trademark rules

"FedNow" is a service mark of the Federal Reserve Banks. Their terms forbid commercial use
or anything implying endorsement without written permission, and specifically forbid using
their marks as metatags or hidden text.

**The brand is Pacsmith.** The mark may appear **only in visible body text, used
descriptively** — "an open-source ISO 20022 toolkit for the FedNow® Service". It must never
appear in:

- `<title>`, `<meta name="description">`, meta keywords;
- Open Graph or Twitter card tags, JSON-LD, canonical URLs;
- `alt` text written for SEO;
- URLs or slugs;
- the site name.

Every page also carries the non-affiliation footer, rendered site-wide by
`src/components/Footer.astro`. No Federal Reserve logos, colours or imagery are used, and
the site makes no claim of certification, compatibility or production approval.

`npm run check` enforces the mechanical half of this against the built output: no mark
inside `<head>` on any page, the footer on every page, no mark in any built path, no
analytics, and no unevaluated MDX expressions in links. **Run it after every build.**

It also runs `npm audit --omit=dev --audit-level=high` first, so a high-severity advisory
in a runtime dependency fails the same command that guards the trademark rules. That step
needs network access; `bash scripts/check-build.sh` runs the output checks alone if you are
offline.

When adding a page: put the mark-free wording in `title`/`description`, and the descriptive
wording in the body.

## Deployment

Vercel, via its Git integration — there is no workflow file and no action to pin, because
nothing about the deploy runs in this repository's CI. Settings are in `vercel.json`, plus
two project-level settings that a file cannot express:

- **Root Directory**: `site`
- **Include files outside the root directory in the build step**: **enabled** — required,
  because the sync reads the repository's markdown from `..`. Without it the build fails
  with an explicit message rather than publishing an empty site.

Canonical URL is `https://pacsmith.org` (`site.config.mjs`). **The domain is not registered
yet**; nothing here configures DNS. Until it exists, Vercel's generated URL serves the site
and the canonical tags point at a host that does not resolve — harmless, and corrected when
DNS is attached. `pacsmith.dev` is intended only as a redirect to `.org` and is not
referenced by the build. Override with `SITE_URL` in the Vercel project if needed.

## Analytics

None. The site sets no cookies and makes no third-party requests except the OpenSSF
Scorecard badge on `/evaluate/`. See the `TODO(analytics)` in `astro.config.mjs` before
adding any — the choice has to be cookieless and must not need a consent banner.
