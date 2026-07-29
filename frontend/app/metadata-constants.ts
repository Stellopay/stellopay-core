/**
 * metadata-constants.ts
 *
 * Single source of truth for all site-level metadata values.
 * Consumed by:
 *   - app/layout.tsx  → Next.js Metadata API (Open Graph, Twitter, etc.)
 *   - app/components/JsonLd.tsx → JSON-LD structured data script tag
 *
 * Keep all string literals here so search-console changes need only one edit.
 */

/** Canonical public URL of the Stellopay website. No trailing slash. */
export const SITE_URL = "https://stellopay.xyz";

/** Primary display name used in every schema and OG tag. */
export const SITE_NAME = "Stellopay";

/** One-line description for meta description and schema description fields. */
export const SITE_DESCRIPTION =
  "Stellopay — Soroban-powered payroll infrastructure on Stellar. " +
  "Automate salary disbursement, escrow, multi-currency flows, and compliance on-chain.";

/**
 * Absolute URL to the organisation logo.
 * Must be 112 × 112 px minimum, square, accessible without authentication.
 * Google recommends PNG or SVG at 1:1 aspect ratio for knowledge panels.
 */
export const LOGO_URL = `${SITE_URL}/logo.png`;

/** Twitter / X handle — omit the leading @ */
export const TWITTER_HANDLE = "stellopay";

// ---------------------------------------------------------------------------
// JSON-LD schema types
// ---------------------------------------------------------------------------

export interface OrganizationSchema {
  "@type": "Organization";
  name: string;
  url: string;
  logo: string;
  description: string;
  sameAs: string[];
}

export interface WebSiteSchema {
  "@type": "WebSite";
  name: string;
  url: string;
  description: string;
  potentialAction: {
    "@type": "SearchAction";
    target: {
      "@type": "EntryPoint";
      urlTemplate: string;
    };
    "query-input": string;
  };
}

export interface JsonLdGraph {
  "@context": "https://schema.org";
  "@graph": [OrganizationSchema, WebSiteSchema];
}

/** Build the validated JSON-LD graph for the landing page. */
export function buildJsonLdGraph(): JsonLdGraph {
  const organization: OrganizationSchema = {
    "@type": "Organization",
    name: SITE_NAME,
    url: SITE_URL,
    logo: LOGO_URL,
    description: SITE_DESCRIPTION,
    sameAs: [
      `https://twitter.com/${TWITTER_HANDLE}`,
      `https://github.com/stellopay`,
    ],
  };

  const website: WebSiteSchema = {
    "@type": "WebSite",
    name: SITE_NAME,
    url: SITE_URL,
    description: SITE_DESCRIPTION,
    potentialAction: {
      "@type": "SearchAction",
      target: {
        "@type": "EntryPoint",
        urlTemplate: `${SITE_URL}/search?q={search_term_string}`,
      },
      "query-input": "required name=search_term_string",
    },
  };

  return {
    "@context": "https://schema.org",
    "@graph": [organization, website],
  };
}
