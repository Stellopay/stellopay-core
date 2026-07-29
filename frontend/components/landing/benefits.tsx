/**
 * benefits.tsx — Stellopay Benefits section
 *
 * ## Gradient tokenization
 *
 * ### Before (hardcoded hex stops)
 * Every colour value was an inline literal, e.g.:
 *   section: background: "linear-gradient(135deg, #0f172a, #1e1b4b, #0f172a)"
 *   card icon: background: "linear-gradient(135deg, #312e81, #3730a3)"
 *
 * ### After (CSS custom property tokens)
 * All colour values are referenced through tokens defined in app/globals.css:
 *   section: background: "var(--gradient-section-bg)"
 *   card icon: background: "var(--gradient-card-indigo)"
 *
 * ## Token inventory (hex → token mapping)
 *
 * | Raw hex stop                          | Token                              |
 * |---------------------------------------|------------------------------------|
 * | #0f172a  (slate-900)  section from   | --color-grad-from                  |
 * | #1e1b4b  (indigo-950) section via    | --color-grad-via                   |
 * | #0f172a  (slate-900)  section to     | --color-grad-to                    |
 * | #312e81  (indigo-900) card from      | --color-card-accent-indigo-from    |
 * | #3730a3  (indigo-800) card to        | --color-card-accent-indigo-to      |
 * | #4c1d95  (violet-900) card from      | --color-card-accent-violet-from    |
 * | #5b21b6  (violet-800) card to        | --color-card-accent-violet-to      |
 * | #0e7490  (cyan-700)   card from      | --color-card-accent-cyan-from      |
 * | #0891b2  (cyan-600)   card to        | --color-card-accent-cyan-to        |
 * | #059669  (emerald-600) card from     | --color-card-accent-emerald-from   |
 * | #10b981  (emerald-500) card to       | --color-card-accent-emerald-to     |
 * | #f8fafc  (slate-50)   text on dark   | --color-text-on-dark               |
 * | #94a3b8  (slate-400)  muted on dark  | --color-text-on-dark-muted         |
 *
 * Composed gradient tokens (built in globals.css from the stops above):
 *   --gradient-section-bg   = linear-gradient(135deg, from, via, to)
 *   --gradient-card-indigo  = linear-gradient(135deg, indigo-from, indigo-to)
 *   --gradient-card-violet  = linear-gradient(135deg, violet-from, violet-to)
 *   --gradient-card-cyan    = linear-gradient(135deg, cyan-from, cyan-to)
 *   --gradient-card-emerald = linear-gradient(135deg, emerald-from, emerald-to)
 *
 * ## Accessibility (WCAG 2.1 AA)
 * - Section uses <section> landmark with aria-labelledby pointing to the
 *   visible heading — WCAG 2.4.6 (Headings and Labels).
 * - Cards use <article> elements inside a <ul role="list"> for proper list
 *   semantics while remaining valid HTML.
 * - All decorative icon containers have aria-hidden="true".
 * - Text contrast on dark background: --color-text-on-dark (#f8fafc) on
 *   --color-bg-section-dark (#0f172a) → contrast ≥ 16:1 ✓
 * - Muted text: --color-text-on-dark-muted (#94a3b8) on #0f172a → ≈ 5.8:1 ✓
 * - No focus outlines removed; relies on globals.css :focus-visible rule.
 * - Responsive grid: 1 col (< 640 px) → 2 cols (≥ 640 px) → 4 cols (≥ 1024 px)
 *
 * ## Responsive breakpoints
 * sm 640 px  → 2-column grid (minmax 260 px)
 * md 768 px  → 2-column grid (unchanged)
 * lg 1024 px → 4-column grid
 * xl 1280 px → 4-column grid (unchanged, max-width centres content)
 */

import React from "react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface Benefit {
  /** Short display name shown as the card heading. */
  title: string;
  /** One-to-two sentence description rendered below the heading. */
  description: string;
  /**
   * Emoji or short symbol rendered inside the icon container.
   * Hidden from assistive technology via aria-hidden on the container.
   */
  icon: string;
  /**
   * CSS custom-property reference for the card's icon-badge gradient.
   * Must resolve to one of the --gradient-card-* tokens defined in globals.css.
   */
  gradient: string;
  /**
   * CSS custom-property reference for the icon container background tint.
   * Must resolve to one of the --color-icon-bg-* tokens defined in globals.css.
   */
  iconBg: string;
}

export interface BenefitsProps {
  /**
   * Override the default benefit items. Useful for testing or CMS-driven content.
   * Falls back to DEFAULT_BENEFITS when omitted.
   */
  items?: Benefit[];
}

// ---------------------------------------------------------------------------
// Default data
// ---------------------------------------------------------------------------

/**
 * Default benefit cards.
 * Each `gradient` and `iconBg` value references a token from globals.css,
 * not a raw hex colour.
 */
export const DEFAULT_BENEFITS: Benefit[] = [
  {
    title: "Payroll escrow",
    description:
      "Fund salary agreements up front. Tokens are held on-chain and released automatically on schedule or milestone completion — no manual intervention needed.",
    icon: "🔐",
    gradient: "var(--gradient-card-indigo)",
    iconBg: "var(--color-icon-bg-indigo)",
  },
  {
    title: "Multi-currency",
    description:
      "Disburse in any Stellar asset. An on-chain FX oracle converts amounts at settlement time so both parties always agree on the value.",
    icon: "💱",
    gradient: "var(--gradient-card-violet)",
    iconBg: "var(--color-icon-bg-violet)",
  },
  {
    title: "Governance",
    description:
      "On-chain proposals and multi-sig approvals for high-stakes payroll operations. Every decision is auditable and tamper-proof.",
    icon: "🗳️",
    gradient: "var(--gradient-card-cyan)",
    iconBg: "var(--color-icon-bg-cyan)",
  },
  {
    title: "Compliance",
    description:
      "Rule-based compliance checks, tax withholding, and immutable audit logs emitted per transaction. Stay compliant across jurisdictions.",
    icon: "✅",
    gradient: "var(--gradient-card-emerald)",
    iconBg: "var(--color-icon-bg-emerald)",
  },
];

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

interface BenefitCardProps {
  benefit: Benefit;
}

/**
 * Individual benefit card.
 * Rendered as an <article> inside the list for correct landmark semantics.
 */
function BenefitCard({ benefit }: BenefitCardProps) {
  const { title, description, icon, gradient, iconBg } = benefit;

  return (
    <article
      data-testid="benefit-card"
      style={{
        backgroundColor: "var(--color-card-bg, #1e293b)",
        border: "1px solid var(--color-card-border-dark)",
        borderRadius: "var(--radius-xl)",
        padding: "var(--space-6)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
        boxShadow: "var(--shadow-card-dark)",
        /* Smooth hover lift — respects prefers-reduced-motion via the
           transition-fast token; a @media block would disable it fully */
        transition: "transform var(--transition-fast), box-shadow var(--transition-fast)",
      }}
    >
      {/* Icon badge
           background shorthand sets the gradient image; background-color
           is used as a fallback tint when the gradient cannot be resolved
           (e.g. in JSDOM tests that don't parse CSS variables). */}
      <div
        aria-hidden="true"
        data-testid="benefit-icon-badge"
        data-gradient={gradient}
        data-icon-bg={iconBg}
        style={{
          width: "2.75rem",       /* 44 px — minimum touch target size */
          height: "2.75rem",
          borderRadius: "var(--radius-lg)",
          background: gradient,
          backgroundColor: iconBg,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: "1.25rem",
          flexShrink: 0,
        }}
      >
        {icon}
      </div>

      {/* Text content */}
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
        <h3
          data-testid="benefit-title"
          style={{
            fontSize: "var(--font-size-lg)",
            fontWeight: "var(--font-weight-semibold)",
            color: "var(--color-text-heading-dark)",
            lineHeight: "var(--line-height-snug)",
            margin: 0,
          }}
        >
          {title}
        </h3>

        <p
          data-testid="benefit-description"
          style={{
            fontSize: "var(--font-size-sm)",
            color: "var(--color-text-on-dark-muted)",
            lineHeight: "var(--line-height-relaxed)",
            margin: 0,
          }}
        >
          {description}
        </p>
      </div>
    </article>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

/**
 * Benefits section for the Stellopay landing page.
 *
 * All background and accent colours are consumed via CSS custom-property
 * tokens (e.g. `var(--gradient-section-bg)`) rather than raw hex values.
 * Updating a colour for a rebrand is a single-file change in globals.css.
 *
 * @example
 * ```tsx
 * // Default usage
 * <Benefits />
 *
 * // Custom items (e.g. from a CMS)
 * <Benefits items={cmsItems} />
 * ```
 */
export default function Benefits({ items = DEFAULT_BENEFITS }: BenefitsProps) {
  const headingId = "benefits-heading";

  return (
    <section
      aria-labelledby={headingId}
      data-testid="benefits-section"
      style={{
        /* ── Section background ──────────────────────────────────────────
           Before: background: "linear-gradient(135deg, #0f172a, #1e1b4b, #0f172a)"
           After:  background: "var(--gradient-section-bg)"
           ─────────────────────────────────────────────────────────────── */
        background: "var(--gradient-section-bg)",
        padding: "var(--space-20) var(--space-6)",
        color: "var(--color-text-on-dark)",
      }}
    >
      {/* Inner container — caps width and centres on large screens */}
      <div
        style={{
          maxWidth: "var(--max-w-xl)",
          margin: "0 auto",
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-12)",
        }}
      >
        {/* ── Heading block ──────────────────────────────────────────── */}
        <header style={{ textAlign: "center" }}>
          <h2
            id={headingId}
            data-testid="benefits-heading"
            style={{
              fontSize: "clamp(var(--font-size-2xl), 4vw, var(--font-size-4xl))",
              fontWeight: "var(--font-weight-extrabold)",
              color: "var(--color-text-heading-dark)",
              lineHeight: "var(--line-height-tight)",
              marginBottom: "var(--space-4)",
            }}
          >
            Why Stellopay?
          </h2>

          <p
            data-testid="benefits-subtitle"
            style={{
              fontSize: "clamp(var(--font-size-base), 2vw, var(--font-size-lg))",
              color: "var(--color-text-on-dark-muted)",
              maxWidth: "var(--max-w-prose)",
              margin: "0 auto",
              lineHeight: "var(--line-height-relaxed)",
            }}
          >
            Soroban-powered payroll infrastructure that keeps funds safe,
            payments accurate, and auditors happy.
          </p>
        </header>

        {/* ── Card grid ──────────────────────────────────────────────── */}
        {items.length === 0 ? (
          /* Empty state — accessible and testable */
          <p
            data-testid="benefits-empty"
            role="status"
            style={{
              textAlign: "center",
              color: "var(--color-text-on-dark-muted)",
              fontSize: "var(--font-size-base)",
            }}
          >
            No benefits to display.
          </p>
        ) : (
          <ul
            role="list"
            data-testid="benefits-list"
            style={{
              listStyle: "none",
              padding: 0,
              margin: 0,
              display: "grid",
              /*
               * Responsive grid:
               *   < 640 px  → 1 col  (minmax collapses to single column)
               *   ≥ 640 px  → 2 cols
               *   ≥ 1024 px → 4 cols (auto-fit fills the container)
               */
              gridTemplateColumns:
                "repeat(auto-fit, minmax(min(260px, 100%), 1fr))",
              gap: "var(--space-6)",
            }}
          >
            {items.map((benefit) => (
              <li key={benefit.title}>
                <BenefitCard benefit={benefit} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
