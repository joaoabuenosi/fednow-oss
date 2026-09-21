// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { SITE_URL, SITE_BASE, REPO_URL, OG_IMAGE, OG_IMAGE_ALT } from './site.config.mjs';

// TRADEMARK RULE (see issue #85 and site/README.md)
// -------------------------------------------------
// "FedNow" is a service mark of the Federal Reserve Banks. It must never appear
// in machine-readable metadata: no <title>, meta description, meta keywords,
// Open Graph / Twitter tags, JSON-LD, SEO alt text, URLs or slugs, and not in
// the site name. Every `title` and `description` below therefore describes the
// project without the mark. The mark appears only in visible body text, used
// descriptively. `npm run check` fails the build if this ever regresses — and
// that now covers the social preview too: the og:/twitter: tags added in `head`
// below, the alt text they carry, and the bytes of the image itself.

export default defineConfig({
  site: SITE_URL,
  base: SITE_BASE,
  trailingSlash: 'always',
  integrations: [
    starlight({
      title: 'Pacsmith',
      description:
        'An open-source ISO 20022 toolkit for US instant payments: message library, simulator, send gateway and SDKs.',
      credits: false,
      social: [{ icon: 'github', label: 'GitHub', href: REPO_URL }],
      // Starlight builds an edit URL as `baseUrl` + the page's path relative to
      // the ASTRO PROJECT ROOT, which here is site/ — not the repository root.
      // Without the `site/` segment every hand-authored page pointed at
      // `edit/main/src/content/docs/...`, which 404s on GitHub. The pages synced
      // by scripts/sync-docs.mjs were never affected: each one writes an
      // explicit `editUrl` into its frontmatter pointing at the repository file
      // it was generated from, which overrides this. Check [9/9] resolves every
      // edit link in the build against the working tree, so a 404 fails the
      // build rather than being found by whoever clicked it.
      editLink: { baseUrl: `${REPO_URL}/edit/main/site/` },
      customCss: ['./src/styles/custom.css'],
      components: {
        // Adds the non-affiliation notice to every page, including 404.
        Footer: './src/components/Footer.astro',
      },
      head: [
        // ------------------------------------------------------------------
        // Social preview (Open Graph / Twitter-X large summary card).
        //
        // Starlight already emits og:title, og:description, og:url and
        // twitter:card; it does not emit an image, so links to the site
        // unfurled as a bare text card. The artwork is original — the
        // project's own wordmark and forge-ember palette, source in
        // src/assets/og-image.svg, rendered by scripts/render-og.sh.
        //
        // TRADEMARK RULE: the mark appears in neither the image nor this alt
        // text. `alt` is the one piece of image metadata a crawler reads, so
        // it is held to the same rule as <title>; check [7/8] enforces it.
        //
        // The URL must be absolute: crawlers do not resolve a relative
        // og:image against the page they found it on.
        { tag: 'meta', attrs: { property: 'og:image', content: OG_IMAGE } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { property: 'og:image:alt', content: OG_IMAGE_ALT } },
        { tag: 'meta', attrs: { name: 'twitter:image', content: OG_IMAGE } },
        { tag: 'meta', attrs: { name: 'twitter:image:alt', content: OG_IMAGE_ALT } },

        // ------------------------------------------------------------------
        // Vercel Web Analytics — page-view counts, cookieless and first-party.
        //
        // This replaces an earlier TODO here that said not to enable it
        // without checking what it stores first. That check was done, against
        // Vercel's current Web Analytics privacy documentation and against the
        // shipped @vercel/analytics source:
        //
        //   * No cookies and no client-side storage. Visitors are distinguished
        //     by a hash Vercel derives server-side from the incoming request,
        //     which is discarded after 24 hours. Nothing is written to the
        //     browser, so no consent banner is required.
        //   * First-party. Both the script and the endpoint it reports to are
        //     served from this site's own origin under /_vercel/insights/ — no
        //     request to any third-party host. (The npm package swaps in a
        //     va.vercel-scripts.com debug script in development; that is
        //     exactly why the two plain tags below are used instead of the
        //     package, which also keeps the site's dependency count at two.)
        //   * No personal data or IP addresses are stored or made available.
        //
        // What the site may therefore claim is written out in full on
        // /evaluate/ and /about/, and the wording there is deliberately narrow.
        // If this ever changes, those pages change with it — check [4/8] fails
        // the build if the pages still carry the old "loads no analytics" line
        // while this script is present.
        //
        // NOTE: /_vercel/insights/script.js is served by Vercel only once Web
        // Analytics is enabled for the project in the Vercel dashboard. Until
        // then these tags are inert and the request 404s, which is harmless.
        {
          tag: 'script',
          content: 'window.va=window.va||function(){(window.vaq=window.vaq||[]).push(arguments)};',
        },
        { tag: 'script', attrs: { defer: true, src: '/_vercel/insights/script.js' } },
      ],
      sidebar: [
        {
          label: 'Build',
          items: [
            { label: 'Overview', slug: 'docs/overview' },
            { label: 'Quick start', slug: 'docs/quickstart' },
            {
              label: 'Integration handbook',
              items: [
                { label: 'All chapters', slug: 'docs/handbook' },
                { label: '1 — Credit transfer flow', slug: 'docs/handbook/credit-transfer-flow' },
                { label: '2 — Timeout reconciliation', slug: 'docs/handbook/timeout-reconciliation' },
                { label: '4 — Zero to certification testing', slug: 'docs/handbook/zero-to-ctp' },
                { label: '5 — Returns', slug: 'docs/handbook/returns' },
              ],
            },
            { label: 'Gateway API', slug: 'docs/gateway-api' },
            {
              label: 'SDKs',
              items: [
                { label: 'Python', slug: 'docs/sdks/python' },
                { label: 'Java', slug: 'docs/sdks/java' },
              ],
            },
            { label: 'Simulator', slug: 'docs/simulator' },
            { label: 'Conformance suite', slug: 'docs/conformance' },
          ],
        },
        {
          label: 'Evaluate',
          items: [
            { label: 'Start here', slug: 'evaluate' },
            { label: 'Security policy', slug: 'evaluate/security-policy' },
            { label: 'Licence', slug: 'evaluate/licence' },
          ],
        },
        {
          label: 'About',
          items: [{ label: 'About the project', slug: 'about' }],
        },
      ],
    }),
  ],
});
