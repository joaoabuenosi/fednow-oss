// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { SITE_URL, SITE_BASE, REPO_URL } from './site.config.mjs';

// TRADEMARK RULE (see issue #85 and site/README.md)
// -------------------------------------------------
// "FedNow" is a service mark of the Federal Reserve Banks. It must never appear
// in machine-readable metadata: no <title>, meta description, meta keywords,
// Open Graph / Twitter tags, JSON-LD, SEO alt text, URLs or slugs, and not in
// the site name. Every `title` and `description` below therefore describes the
// project without the mark. The mark appears only in visible body text, used
// descriptively. `npm run check` fails the build if this ever regresses.

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
      editLink: { baseUrl: `${REPO_URL}/edit/main/` },
      customCss: ['./src/styles/custom.css'],
      components: {
        // Adds the non-affiliation notice to every page, including 404.
        Footer: './src/components/Footer.astro',
      },
      // TODO(analytics): no analytics are loaded today, by design — the site
      // sets no cookies and makes no third-party requests. When we want
      // numbers, pick a cookieless, no-personal-data option (e.g. a
      // self-hosted GoatCounter or Plausible instance) and wire it through
      // `head` here, with the choice documented on /about/. Do not add
      // anything that needs a consent banner, and do not enable Vercel Web
      // Analytics without checking what it stores first.
      head: [],
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
