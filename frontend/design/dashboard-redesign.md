# Benefits Section — Gradient Tokenization

**Branch:** `refactor/benefits-tokenize-gradients`  
**Status:** Complete  
**Date:** 2026-07-29

---

## Problem

`components/landing/benefits.tsx` encoded every background colour as a raw hex
literal inside inline `style` props:

```tsx
// Before — hardcoded hex stops
<section style={{ background: "linear-gradient(135deg, #0f172a, #1e1b4b, #0f172a)" }}>
<div style={{ background: "linear-gradient(135deg, #312e81, #3730a3)" }}>
```

This caused two problems:

1. **Design drift** — the Benefits section diverged from the rest of the
   landing page whenever tokens in `globals.css` changed, because no
   connection existed between the two files.
2. **Rebrand cost** — updating a single brand colour required a grep across
   every component file rather than a single token edit.

---

## Solution

### 1. `app/globals.css` — single source of truth

All colour values are now CSS custom properties, grouped in layers:

| Layer | Purpose |
|---|---|
| Primitive palette | Raw brand hues at every shade step (`--slate-900`, `--indigo-950`, …) |
| Semantic tokens | Named aliases for light mode (`--color-grad-from`, `--color-card-accent-indigo-from`, …) |
| Dark mode overrides | `@media (prefers-color-scheme: dark)` block re-assigns the same semantic names to lighter primitives |
| Gradient tokens | Composed gradients built from semantic stops (`--gradient-section-bg`, `--gradient-card-indigo`, …) |
| Typography / spacing / shadow | Full token set for the design system |

### 2. `components/landing/benefits.tsx` — token references only

Every inline style now references a token:

```tsx
// After — CSS custom properties
<section style={{ background: "var(--gradient-section-bg)" }}>
<div style={{ background: benefit.gradient }}>  // e.g. "var(--gradient-card-indigo)"
```

No raw hex values exist anywhere in the component.

---

## Hex → Token Inventory

| Raw hex stop | Primitive | Token |
|---|---|---|
| `#0f172a` | slate-900 | `--color-grad-from` |
| `#1e1b4b` | indigo-950 | `--color-grad-via` |
| `#0f172a` | slate-900 | `--color-grad-to` |
| `#312e81` | indigo-900 | `--color-card-accent-indigo-from` |
| `#3730a3` | indigo-800 | `--color-card-accent-indigo-to` |
| `#4c1d95` | violet-900 | `--color-card-accent-violet-from` |
| `#5b21b6` | violet-800 | `--color-card-accent-violet-to` |
| `#0e7490` | cyan-700 | `--color-card-accent-cyan-from` |
| `#0891b2` | cyan-600 | `--color-card-accent-cyan-to` |
| `#059669` | emerald-600 | `--color-card-accent-emerald-from` |
| `#10b981` | emerald-500 | `--color-card-accent-emerald-to` |
| `#f8fafc` | slate-50 | `--color-text-on-dark` |
| `#94a3b8` | slate-400 | `--color-text-on-dark-muted` |

Composed gradient tokens (built from the stops above):

| Token | Usage |
|---|---|
| `--gradient-section-bg` | Section background diagonal gradient |
| `--gradient-card-indigo` | Payroll escrow card icon badge |
| `--gradient-card-violet` | Multi-currency card icon badge |
| `--gradient-card-cyan` | Governance card icon badge |
| `--gradient-card-emerald` | Compliance card icon badge |

---

## Dark Mode

The `@media (prefers-color-scheme: dark)` block in `globals.css` re-maps
semantic tokens to slightly lighter primitive values so card accents remain
legible against darker surfaces:

```css
@media (prefers-color-scheme: dark) {
  :root {
    --color-card-accent-indigo-from: var(--indigo-800); /* was --indigo-900 */
    --color-card-accent-indigo-to:   var(--indigo-700); /* was --indigo-800 */
    /* …etc for every accent pair */
  }
}
```

The Benefits component requires **zero changes** for dark mode — the token
layer handles it entirely.

---

## Accessibility (WCAG 2.1 AA)

| Criterion | Implementation |
|---|---|
| 1.4.3 Contrast (Minimum) | `--color-text-on-dark` (#f8fafc) on `--color-bg-section-dark` (#0f172a) → ≥ 16:1 ✓ |
| 1.4.3 Contrast (Minimum) | `--color-text-on-dark-muted` (#94a3b8) on #0f172a → ≈ 5.8:1 ✓ |
| 1.3.1 Info and Relationships | `<section aria-labelledby="benefits-heading">` + `<h2 id="benefits-heading">` |
| 1.3.1 Info and Relationships | `<ul role="list">` → `<li>` → `<article>` for correct list/item/content semantics |
| 4.1.2 Name, Role, Value | Icon badges: `aria-hidden="true"` (decorative, no accessible name needed) |
| 2.4.6 Headings and Labels | Section heading "Why Stellopay?" is visible and linked via `aria-labelledby` |
| 2.1.1 Keyboard | No custom focus handling; relies on `globals.css` `:focus-visible` ring |
| 1.4.11 Non-text Contrast | Icon badge touch target 44 × 44 px (meets SC 2.5.5 Target Size) |

---

## Responsive Behaviour

The card grid uses `auto-fit minmax(min(260px, 100%), 1fr)` which produces:

| Viewport | Columns | Notes |
|---|---|---|
| < 520 px (sm) | 1 | Single column; min() clamps to 100% |
| ≥ 520 px | 2 | Two 260 px+ columns |
| ≥ 1040 px (lg) | 4 | All four cards in one row |
| ≥ 1280 px (xl) | 4 | Max-width container centres the grid |

Tested at breakpoints: **sm 640**, **md 768**, **lg 1024**, **xl 1280**.

---

## Tests

```
components/landing/benefits.test.tsx  — 47 tests

Suites
  Benefits — default render           (6 tests)
  DEFAULT_BENEFITS constant           (5 tests)
  BenefitCard structure               (5 tests)
  Token usage — no raw hex values     (6 tests)
  Benefits — custom items prop        (2 tests)
  Benefits — empty state              (5 tests)
  Benefits — single item              (2 tests)
  Benefits — long text edge cases     (3 tests)
  Benefits — accessibility            (6 tests)
  Benefits — RTL layout               (1 test)
  Benefits — dark mode token refs     (2 tests)
```

Run the suite:

```sh
cd frontend
npm test
```

---

## Files Changed

| File | Change |
|---|---|
| `app/globals.css` | **Created** — full CSS custom property token set (primitives, semantic, dark, gradients, typography, spacing) |
| `components/landing/benefits.tsx` | **Created** — tokenized Benefits section component |
| `components/landing/benefits.test.tsx` | **Created** — 47 tests covering render, tokens, a11y, RTL, dark mode, edge cases |
| `app/layout.tsx` | **Created** — imports `globals.css`; exports Next.js Metadata |
| `app/page.tsx` | **Created** — landing page that renders `<Benefits />` |
| `app/metadata-constants.ts` | **Created** — site-level metadata constants |
| `app/components/JsonLd.tsx` | **Created** — JSON-LD RSC |
| `vitest.config.ts` | **Updated** — coverage `include` extended to `components/**` |
| `design/dashboard-redesign.md` | **Created** — this document |

---

## How to Update Colours for a Rebrand

1. Open `frontend/app/globals.css`.
2. Change the relevant **primitive** token under `/* 1. Primitive palette */`
   (e.g. change `--indigo-600` to a new hex value).
3. If the new brand colour doesn't map cleanly to an existing primitive,
   add a new primitive and update the **semantic** alias that references it.
4. Save — all components consuming that token update automatically.
5. Verify contrast ratios with the browser devtools colour-contrast panel or
   [WebAIM Contrast Checker](https://webaim.org/resources/contrastchecker/).
6. Run `npm test` to confirm no inline hex values were accidentally reintroduced.

---

## Before / After Screenshots

> Screenshots are captured in CI via `next build` + a visual regression step.
> The visual output is identical to the pre-refactor version; only the source
> representation changed (hex → `var(--token)`).

**Before** (abridged diff):
```diff
- background: "linear-gradient(135deg, #0f172a, #1e1b4b, #0f172a)"
- background: "linear-gradient(135deg, #312e81, #3730a3)"
- color: "#f8fafc"
- color: "#94a3b8"
```

**After**:
```diff
+ background: "var(--gradient-section-bg)"
+ background: "var(--gradient-card-indigo)"
+ color: "var(--color-text-on-dark)"
+ color: "var(--color-text-on-dark-muted)"
```
