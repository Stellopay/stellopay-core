/**
 * page.tsx — Stellopay landing page
 *
 * Renders the Benefits section using the tokenized component, plus
 * JSON-LD structured data for search engines.
 *
 * Accessibility (WCAG 2.1 AA):
 * - Semantic HTML: <header>, <main>, <section>, <nav>, <footer>
 * - Skip navigation link (WCAG 2.4.1)
 * - lang="en" set in layout.tsx (WCAG 3.1.1)
 * - All colour values come from globals.css tokens
 */

import type { Metadata } from "next";
import JsonLd from "./components/JsonLd";
import Benefits from "../components/landing/benefits";
import { buildJsonLdGraph, SITE_NAME, SITE_DESCRIPTION } from "./metadata-constants";

export const metadata: Metadata = {
  title: `${SITE_NAME} — On-Chain Payroll for the Stellar Ecosystem`,
  description: SITE_DESCRIPTION,
  alternates: { canonical: "/" },
};

export default function HomePage() {
  return (
    <>
      <JsonLd graph={buildJsonLdGraph()} />

      {/* Skip navigation — WCAG 2.1 SC 2.4.1 */}
      <a
        href="#main-content"
        className="sr-only"
        style={{ position: "absolute" }}
      >
        Skip to main content
      </a>

      {/* Header */}
      <header
        role="banner"
        style={{
          backgroundColor: "var(--color-bg-section-dark)",
          padding: "var(--space-4) var(--space-6)",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <a
          href="/"
          aria-label={`${SITE_NAME} — return to homepage`}
          style={{
            color: "var(--color-text-on-dark)",
            fontWeight: "var(--font-weight-bold)",
            fontSize: "var(--font-size-xl)",
            textDecoration: "none",
          }}
        >
          {SITE_NAME}
        </a>

        <nav aria-label="Primary navigation">
          <ul
            style={{
              listStyle: "none",
              margin: 0,
              padding: 0,
              display: "flex",
              gap: "var(--space-6)",
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
                    color: "var(--color-text-on-dark-muted)",
                    textDecoration: "none",
                    fontSize: "var(--font-size-sm)",
                  }}
                  {...(href.startsWith("http")
                    ? { target: "_blank", rel: "noopener noreferrer" }
                    : {})}
                >
                  {label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </header>

      {/* Main */}
      <main id="main-content" tabIndex={-1} role="main">
        {/* Hero */}
        <section
          aria-labelledby="hero-heading"
          style={{
            backgroundColor: "var(--color-bg-section-dark)",
            color: "var(--color-text-on-dark)",
            padding: "var(--space-20) var(--space-6)",
            textAlign: "center",
          }}
        >
          <h1
            id="hero-heading"
            style={{
              fontSize: "clamp(var(--font-size-3xl), 5vw, 3.5rem)",
              fontWeight: "var(--font-weight-extrabold)",
              lineHeight: "var(--line-height-tight)",
              marginBottom: "var(--space-5)",
            }}
          >
            On-Chain Payroll for&nbsp;Stellar
          </h1>
          <p
            style={{
              fontSize: "clamp(var(--font-size-base), 2.5vw, var(--font-size-xl))",
              color: "var(--color-text-on-dark-muted)",
              maxWidth: "52ch",
              margin: "0 auto var(--space-8)",
              lineHeight: "var(--line-height-relaxed)",
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
              backgroundColor: "var(--color-brand-500)",
              color: "var(--color-text-on-accent)",
              padding: "var(--space-3) var(--space-8)",
              borderRadius: "var(--radius-md)",
              textDecoration: "none",
              fontWeight: "var(--font-weight-semibold)",
              fontSize: "var(--font-size-base)",
            }}
          >
            View on GitHub
          </a>
        </section>

        {/* Benefits — tokenized gradient component */}
        <Benefits />
      </main>

      {/* Footer */}
      <footer
        role="contentinfo"
        style={{
          backgroundColor: "var(--color-bg-section-dark)",
          color: "var(--color-text-on-dark-muted)",
          padding: "var(--space-8) var(--space-6)",
          textAlign: "center",
          fontSize: "var(--font-size-sm)",
        }}
      >
        <p>
          &copy; {new Date().getFullYear()} {SITE_NAME}. Open source under the MIT License.
        </p>
      </footer>
    </>
  );
}
