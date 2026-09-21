// Shared site identity — imported by astro.config.mjs and by scripts/sync-docs.mjs
// so the deployed URL is defined exactly once.
//
// Brand: Pacsmith. Canonical home: https://pacsmith.org.
//
// NEITHER DOMAIN IS REGISTERED YET. Nothing here registers, configures or
// assumes DNS for one: `site` only feeds Astro's canonical URL, <link
// rel="canonical">, og:url and the sitemap. Until the domain exists, Vercel
// serves the site on its generated *.vercel.app preview/production URL and
// those absolute URLs simply point at a host that does not resolve yet — which
// is harmless, and is corrected the moment DNS is attached.
//
// pacsmith.dev is intended only as a redirect to pacsmith.org; it is never the
// canonical URL and is not referenced anywhere in the build.
//
// To point the build somewhere else (a preview host, or the .vercel.app URL),
// set SITE_URL in the Vercel project's environment variables. SITE_BASE exists
// for a future move to a path-prefixed host; on Vercel the site is served at
// the domain root, so it is empty.

export const SITE_URL = process.env.SITE_URL ?? 'https://pacsmith.org';
export const SITE_BASE = process.env.SITE_BASE ?? '';

/**
 * The social preview card, as an ABSOLUTE URL.
 *
 * Open Graph and Twitter/X crawlers do not resolve a relative og:image against
 * the page they found it on, so this has to be absolute — and it therefore
 * tracks SITE_URL, including when SITE_URL is overridden to a preview host.
 *
 * Source artwork: src/assets/og-image.svg, rendered to public/og-image.png by
 * scripts/render-og.sh. Both are committed; crawlers do not render SVG.
 *
 * TRADEMARK RULE: OG_IMAGE_ALT is the alt text a crawler reads, which makes it
 * metadata, which means the FedNow mark must not appear in it — same rule as
 * <title>. scripts/check-build.sh enforces that over the built pages.
 */
export const OG_IMAGE = `${SITE_URL.replace(/\/$/, '')}${SITE_BASE.replace(/\/$/, '')}/og-image.png`;
export const OG_IMAGE_ALT =
  'Pacsmith — an open-source ISO 20022 toolkit for US instant payments';

/** The repository the docs are synced from — the single source of truth. */
export const REPO = 'joaoabuenosi/fednow-oss';
export const REPO_URL = `https://github.com/${REPO}`;

/**
 * Prefix an internal, root-relative path with the deploy base path.
 *
 * Used by scripts/sync-docs.mjs, which rewrites the repository's relative
 * markdown links into site URLs. Hand-authored pages under src/content/docs/
 * write plain root-relative paths instead: MDX does not evaluate `{...}` inside
 * a markdown link destination, so a helper call there would ship verbatim as a
 * broken href. scripts/check-build.sh fails the build if one ever does.
 */
export function href(path) {
  const base = SITE_BASE.replace(/\/$/, '');
  return `${base}${path}`;
}
