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

## Social preview

`src/assets/og-image.svg` is the source artwork for the Open Graph / Twitter-X card;
`public/og-image.png` is what crawlers actually fetch, because none of them render SVG.
Both are committed, and `scripts/render-og.sh` is what keeps them in step:

```sh
bash scripts/render-og.sh                        # finds a chromium on PATH
CHROMIUM=/path/to/chrome bash scripts/render-og.sh
```

It is deliberately **not** part of `npm run build` — it needs a Chromium binary, which the
site does not otherwise depend on. Edit the SVG, re-run it, commit the regenerated PNG.

The artwork is original: the project's wordmark, an anvil, and the forge-ember accent from
`custom.css`. No payment operator's mark, logo, imagery or colours appear in it, and the
FedNow mark appears in neither the image nor its alt text — the alt text is the one piece
of image metadata a crawler reads, so it is held to the same rule as `<title>`. The
absolute URL and the alt text both live in `site.config.mjs` (`OG_IMAGE`, `OG_IMAGE_ALT`),
and the tags are emitted from `head` in `astro.config.mjs`.

Two failure modes are checked rather than trusted, because neither shows up in a diff:
a render cropped by Chromium's "new" headless mode (which lays out in a viewport shorter
than `--window-size`), and a PNG that is not 1200x630. `scripts/verify-og.mjs` re-reads the
committed PNG and fails on either — from `render-og.sh` and again from `npm run check`.

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

`npm run check` enforces the mechanical half of this against the built output, in nine
checks: no mark inside `<head>` on any page, the footer on every page, no mark in any built
path, analytics that is first-party and cookieless (below), no unevaluated MDX expressions
in links, no retired ABA routing numbers, no mark in any `og:`/`twitter:` tag or `alt`
attribute **anywhere in the document** (not just `<head>`, which is where checks 1 and 7
differ), no mark in the bytes of the social preview image itself, and every "Edit page"
link resolving to a file that exists (below). **Run it after every build.**

It also runs `npm audit --omit=dev --audit-level=high` first, so a high-severity advisory
in a runtime dependency fails the same command that guards the trademark rules. That step
needs network access; `bash scripts/check-build.sh` runs the output checks alone if you are
offline.

When adding a page: put the mark-free wording in `title`/`description`, and the descriptive
wording in the body.

## "Edit page" links

Starlight builds an edit URL as `editLink.baseUrl` plus the page's path relative to the
**Astro project root**, which here is `site/` — not the repository root. `baseUrl`
therefore ends in `/edit/main/site/`. Pages synced by `scripts/sync-docs.mjs` are
unaffected either way: each writes its own `editUrl` frontmatter pointing at the repository
file it was generated from.

Three pages opt out with `editUrl: false` in their frontmatter — the home page, `/about/`
and `/evaluate/`. Those are the project's own statements rather than documentation to
crowd-edit, and an edit button on a page that procurement and risk read next to the licence
and the disclosure process reads oddly. Everything under Build keeps its link: that is the
standard invitation to contribute, and GitHub routes a non-maintainer through a fork and a
pull request anyway.

Check `[9/9]` resolves every edit link in the build back to a file in the working tree, so
a link that 404s fails the build instead of being discovered by whoever clicked it. It also
fails if *every* edit link disappears, which would mean the Build pages lost theirs by
accident.

## Maintainer's personal data

`src/content/docs/about.md` is the only page that states anything about the maintainer, and
its frontmatter carries the rule: name, GitHub handle and the one line of background, and
nothing beyond them without the maintainer's own words. See **AGENTS.md → Rules →
Maintainer's personal data** in the repository root; keep the two in step.

## Deployment

Vercel, via its Git integration — there is no workflow file and no action to pin, because
nothing about the deploy runs in this repository's CI. Settings are in `vercel.json`, plus
two project-level settings that a file cannot express:

- **Root Directory**: `site`
- **Include files outside the root directory in the build step**: **enabled** — required,
  because the sync reads the repository's markdown from `..`. Without it the build fails
  with an explicit message rather than publishing an empty site.
- **Web Analytics**: **enable it** (Project → Analytics), or the first-party
  `/_vercel/insights/` route is not served and the page-view count silently stays empty.
  See "Analytics" above.

Canonical URL is `https://pacsmith.org` (`site.config.mjs`). **The domain is not registered
yet**; nothing here configures DNS. Until it exists, Vercel's generated URL serves the site
and the canonical tags point at a host that does not resolve — harmless, and corrected when
DNS is attached. `pacsmith.dev` is intended only as a redirect to `.org` and is not
referenced by the build. Override with `SITE_URL` in the Vercel project if needed.

## Analytics

**Vercel Web Analytics**, loaded from two plain script tags in `head` in
`astro.config.mjs`. It replaced a `TODO(analytics)` there which said not to enable it
without first checking what it stores. That check was done, against Vercel's current Web
Analytics privacy documentation and against the shipped `@vercel/analytics` source:

- **No cookies.** Visitors are distinguished by a hash Vercel derives server-side from the
  incoming request, discarded after 24 hours. Nothing is written to the browser, so no
  consent banner is required.
- **First-party.** Both the script and the endpoint it reports to are served from this
  site's own origin under `/_vercel/insights/`. No request goes to any third-party host.
- **No personal data or IP addresses** are stored or made available.

The npm package is deliberately **not** used. It swaps in a `va.vercel-scripts.com` debug
script outside production — a third-party request the site does not want — and two script
tags keep the site's dependency count at two. Check `[4/8]` fails the build if that host,
or any other tracker, ever appears.

### The site's privacy claims are checked, not asserted

`/evaluate/`, `/about/#privacy` and `public/robots.txt` each state exactly what the site
does. `npm run check` holds them to it:

- no third-party tracker, and every `/_vercel/insights` reference root-relative;
- nothing touches `document.cookie`, anywhere;
- browser storage limited to the two keys `/about/#privacy` names in full —
  `starlight-theme` (`localStorage`) and `sl-sidebar-state` (`sessionStorage`), both
  written by Starlight to remember the theme and sidebar you left, both purely local. A
  third key fails the build until the table on `/about/` is updated;
- and the analytics script and the prose must agree: if the script is present while any
  page still carries the old "loads no analytics" wording, the build fails. **The site must
  never make a claim that is not exactly true — that is the check that enforces it.**

### Maintainer step, outside this repository

`/_vercel/insights/script.js` is served by Vercel only once **Web Analytics is enabled for
the project in the Vercel dashboard** (Project → Analytics → Enable). Until then the tags
are inert and the request 404s, which is harmless. Nothing in this repository can turn it
on.
