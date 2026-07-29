/**
 * JsonLd.tsx
 *
 * A zero-dependency React Server Component that injects a
 * <script type="application/ld+json"> tag into the document <head>.
 *
 * Design decisions:
 * - Server Component only: no "use client" — script tags belong in the head,
 *   not managed by client-side React. This avoids hydration mismatches.
 * - dangerouslySetInnerHTML is intentional and safe here because the payload
 *   is constructed entirely from compile-time constants, never from user input.
 * - The component uses JSON.stringify with no reviver, producing a compact but
 *   parseable string that Google's Rich Results Test accepts.
 *
 * Accessibility notes (WCAG 2.1 AA):
 * - <script> elements are invisible to AT; no ARIA role needed.
 * - The type="application/ld+json" attribute is required — without it some
 *   screen readers may attempt to voice the raw JSON text.
 * - No visual output means no contrast, keyboard, or focus considerations.
 *
 * @see https://developers.google.com/search/docs/appearance/structured-data/intro-structured-data
 */

import type { JsonLdGraph } from "../metadata-constants";

interface JsonLdProps {
  /** Pre-built JSON-LD graph from buildJsonLdGraph(). */
  graph: JsonLdGraph;
}

/**
 * Renders a single <script type="application/ld+json"> tag containing the
 * supplied graph payload.  Must be rendered inside <head> (place it in
 * layout.tsx or page.tsx — Next.js 13+ App Router hoists head children
 * automatically).
 *
 * Example:
 * ```tsx
 * import { buildJsonLdGraph } from "@/app/metadata-constants";
 * import JsonLd from "@/app/components/JsonLd";
 *
 * export default function Page() {
 *   return (
 *     <>
 *       <JsonLd graph={buildJsonLdGraph()} />
 *       <main>…</main>
 *     </>
 *   );
 * }
 * ```
 */
export default function JsonLd({ graph }: JsonLdProps) {
  // Serialize to compact JSON (no trailing newline) for minimal byte size.
  // The payload is never user-supplied so XSS via dangerouslySetInnerHTML
  // is not a concern, but we still use JSON.stringify to ensure valid JSON.
  const json = JSON.stringify(graph);

  return (
    <script
      type="application/ld+json"
      // eslint-disable-next-line react/no-danger
      dangerouslySetInnerHTML={{ __html: json }}
    />
  );
}
