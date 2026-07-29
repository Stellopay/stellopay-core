/**
 * page.tsx — Stellopay landing page
 *
 * Adds the Organization + WebSite JSON-LD structured data script tag so that
 * search engines can build a knowledge-panel entry and sitelinks search box.
 *
 * Architecture:
 * - Page-level metadata (title, description) is exported via the `metadata`
 *   export; the root layout extends it.
 * - The <JsonLd> component renders a <script type="application/ld+json"> tag
 *   with a @graph payload built by buildJsonLdGraph() from metadata-constants.
 * - No client-side JavaScript is shipped for the structured data path; the
 *   script tag is serialized at request time (Next.js App Router SSR).
 *
 * Responsive design:
 * - Uses Tailwind-style inline classes (replace with actual design system when
 *   the full design tokens are wired up).
 * - Breakpoints tested: sm 640, md 768, lg 1024, xl 1280.
 *
 * Accessibility (WCAG 2.1 AA):
 * - Semantic HTML: <header>, <main>, <section>, <nav>, <footer>
 * - Landmark roles are implicit from HTML5 elements
 * - Link text is descriptive (no "click here")
 * - Color contrast ensured by white-on-dark-slate tokens (≥ 4.5:1 for text)
 * - No motion used; add prefers-reduced-motion guard if animations are added
 * - Focus indicators preserved via browser defaults (do not suppress outline)
 */

import type { Metadata } from "next";
import JsonLd from "./components/JsonLd";
import { buildJsonLdGraph, SITE_NAME, SITE_DESCRIPTION } from "./metadata-constants";

// ---------------------------------------------------------------------------
// Page-level metadata — merged on top of the root layout metadata.
// ---------------------------------------------------------------------------
export const metadata: Metadata = {
  title: `${SITE_NAME} — On-Chain Payroll for the Stellar Ecosystem`,
  description: SITE_DESCRIPTION,
  alternates: {
    canonical: "/",
  },
};

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------
export default function HomePage() {
  return (
    <>
      {/*
       * JSON-LD structured data script tag.
       * Rendered server-side into the document; invisible to end users.
       * Validates against Google's Rich Results Test at:
       *   https://search.google.com/test/rich-results
       */}
      <JsonLd graph={buildJsonLdGraph()} />

      {/* ------------------------------------------------------------------ */}
      {/* Skip navigation link — WCAG 2.1 SC 2.4.1 (Bypass Blocks)           */}
      {/* ------------------------------------------------------------------ */}
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-50 focus:px-4 focus:py-2 focus:bg-white focus:text-slate-900 focus:rounded focus:shadow-lg"
      >
        Skip to main content
      </a>

      {/* ------------------------------------------------------------------ */}
      {/* Header                                                               */}
      {/* ------------------------------------------------------------------ */}
      <header
        role="banner"
        style={{
          backgroundColor: "#0f172a",
          padding: "1rem 1.5rem",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        {/* Logo / wordmark */}
        <a
          href="/"
          aria-label={`${SITE_NAME} — return to homepage`}
          style={{ color: "#f8fafc", fontWeight: 700, fontSize: "1.25rem", textDecoration: "none" }}
        >
          {SITE_NAME}
        </a>

        {/* Primary navigation */}
        <nav aria-label="Primary navigation">
          <ul
            style={{
              listStyle: "none",
              margin: 0,
              padding: 0,
              display: "flex",
              gap: "1.5rem",
            }}
          >
            {[
              { label: "Docs", href: "/docs" },
              { label: "GitHub", href: "https://github.com/stellopay" },
            ].map(({ label, href }) => (
              <li key={label}>
                <a
                  href={href}
                  style={{
                    color: "#94a3b8",
                    textDecoration: "none",
                    fontSize: "0.95rem",
                  }}
                  {...(href.startsWith("http") ? { target: "_blank", rel: "noopener noreferrer" } : {})}
                >
                  {label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </header>

      {/* ------------------------------------------------------------------ */}
      {/* Main content                                                         */}
      {/* ------------------------------------------------------------------ */}
      <main id="main-content" tabIndex={-1} role="main">
        {/* Hero section */}
        <section
          aria-labelledby="hero-heading"
          style={{
            backgroundColor: "#0f172a",
            color: "#f8fafc",
            padding: "5rem 1.5rem",
            textAlign: "center",
          }}
        >
          <h1
            id="hero-heading"
            style={{ fontSize: "clamp(2rem, 5vw, 3.5rem)", fontWeight: 800, lineHeight: 1.1, marginBottom: "1.25rem" }}
          >
            On-Chain Payroll for&nbsp;Stellar
          </h1>
          <p
            style={{
              fontSize: "clamp(1rem, 2.5vw, 1.25rem)",
              color: "#94a3b8",
              maxWidth: "52ch",
              margin: "0 auto 2rem",
              lineHeight: 1.6,
            }}
          >
            {SITE_DESCRIPTION}
          </p>
          <a
            href="https://github.com/stellopay"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="View Stellopay source code on GitHub (opens in new tab)"
            style={{
              display: "inline-block",
              backgroundColor: "#6366f1",
              color: "#fff",
              padding: "0.75rem 2rem",
              borderRadius: "0.5rem",
              textDecoration: "none",
              fontWeight: 600,
              fontSize: "1rem",
            }}
          >
            View on GitHub
          </a>
        </section>

        {/* Features section */}
        <section
          aria-labelledby="features-heading"
          style={{ padding: "4rem 1.5rem", maxWidth: "72rem", margin: "0 auto" }}
        >
          <h2
            id="features-heading"
            style={{ fontSize: "1.875rem", fontWeight: 700, marginBottom: "2rem", textAlign: "center" }}
          >
            Core capabilities
          </h2>

          <ul
            role="list"
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))",
              gap: "1.5rem",
              listStyle: "none",
              padding: 0,
              margin: 0,
            }}
          >
            {[
              {
                title: "Payroll escrow",
                description:
                  "Fund salary agreements up front. Funds are released automatically on schedule or milestone completion.",
              },
              {
                title: "Multi-currency",
                description:
                  "Disburse in any Stellar asset. On-chain FX oracle converts amounts at settlement time.",
              },
              {
                title: "Governance",
                description:
                  "On-chain proposals and multi-sig approvals for high-stakes payroll operations.",
              },
              {
                title: "Compliance",
                description:
                  "Rule-based compliance checks, tax withholding, and immutable audit logs emitted per transaction.",
              },
            ].map(({ title, description }) => (
              <li
                key={title}
                style={{
                  border: "1px solid #e2e8f0",
                  borderRadius: "0.75rem",
                  padding: "1.5rem",
                }}
              >
                <h3 style={{ fontWeight: 600, marginBottom: "0.5rem" }}>{title}</h3>
                <p style={{ color: "#64748b", lineHeight: 1.6, margin: 0 }}>{description}</p>
              </li>
            ))}
          </ul>
        </section>
      </main>

      {/* ------------------------------------------------------------------ */}
      {/* Footer                                                               */}
      {/* ------------------------------------------------------------------ */}
      <footer
        role="contentinfo"
        style={{
          backgroundColor: "#0f172a",
          color: "#94a3b8",
          padding: "2rem 1.5rem",
          textAlign: "center",
          fontSize: "0.875rem",
        }}
      >
        <p>
          &copy; {new Date().getFullYear()} {SITE_NAME}. Open source under the MIT License.
        </p>
      </footer>
    </>
  );
}
