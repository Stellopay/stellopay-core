/**
 * metadata.test.tsx
 *
 * Tests for the structured data pipeline:
 *   metadata-constants.ts  →  buildJsonLdGraph()  →  <JsonLd> component
 *
 * Coverage goals:
 * - Constants have the expected shape and non-empty values
 * - buildJsonLdGraph() returns a valid @graph with both schemas
 * - All required JSON-LD fields are present and correctly typed
 * - @context appears exactly ONCE at the top level (no duplicates in sub-nodes)
 * - The output serializes to valid JSON and back without data loss
 * - <JsonLd> renders a <script type="application/ld+json"> tag
 * - Edge cases: idempotent calls, no mutation between calls
 *
 * Run:  npm test  (from frontend/)
 */

import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import {
  SITE_URL,
  SITE_NAME,
  SITE_DESCRIPTION,
  LOGO_URL,
  TWITTER_HANDLE,
  buildJsonLdGraph,
  type JsonLdGraph,
  type OrganizationSchema,
  type WebSiteSchema,
} from "./metadata-constants";
import JsonLd from "./components/JsonLd";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

describe("metadata constants", () => {
  it("SITE_URL is an absolute HTTPS URL without a trailing slash", () => {
    expect(SITE_URL).toMatch(/^https:\/\//);
    expect(SITE_URL).not.toMatch(/\/$/);
  });

  it("SITE_NAME is a non-empty string", () => {
    expect(typeof SITE_NAME).toBe("string");
    expect(SITE_NAME.trim().length).toBeGreaterThan(0);
  });

  it("SITE_DESCRIPTION is a non-empty string", () => {
    expect(typeof SITE_DESCRIPTION).toBe("string");
    expect(SITE_DESCRIPTION.trim().length).toBeGreaterThan(0);
  });

  it("LOGO_URL starts with SITE_URL", () => {
    expect(LOGO_URL).toMatch(/^https:\/\//);
    expect(LOGO_URL.startsWith(SITE_URL)).toBe(true);
  });

  it("TWITTER_HANDLE does not contain a leading @", () => {
    expect(TWITTER_HANDLE).not.toMatch(/^@/);
    expect(TWITTER_HANDLE.trim().length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// buildJsonLdGraph — shape validation
// ---------------------------------------------------------------------------

describe("buildJsonLdGraph()", () => {
  let graph: JsonLdGraph;

  it("returns an object without throwing", () => {
    expect(() => {
      graph = buildJsonLdGraph();
    }).not.toThrow();
  });

  it("top-level @context is 'https://schema.org'", () => {
    graph = buildJsonLdGraph();
    expect(graph["@context"]).toBe("https://schema.org");
  });

  it("@graph contains exactly two items", () => {
    graph = buildJsonLdGraph();
    expect(Array.isArray(graph["@graph"])).toBe(true);
    expect(graph["@graph"]).toHaveLength(2);
  });

  // -- No duplicate @context -------------------------------------------------
  // The @graph pattern requires @context to appear only once at the root.
  // Sub-nodes must NOT repeat it — this is the "no duplicate" requirement.

  describe("no duplicate @context (JSON-LD @graph contract)", () => {
    it("serialized JSON contains '@context' exactly once", () => {
      graph = buildJsonLdGraph();
      const json = JSON.stringify(graph);
      const occurrences = (json.match(/"@context"/g) ?? []).length;
      expect(occurrences).toBe(1);
    });

    it("Organization sub-node does not have its own @context key", () => {
      graph = buildJsonLdGraph();
      const org = graph["@graph"][0] as Record<string, unknown>;
      expect(Object.prototype.hasOwnProperty.call(org, "@context")).toBe(false);
    });

    it("WebSite sub-node does not have its own @context key", () => {
      graph = buildJsonLdGraph();
      const site = graph["@graph"][1] as Record<string, unknown>;
      expect(Object.prototype.hasOwnProperty.call(site, "@context")).toBe(false);
    });
  });

  // -- Organization ----------------------------------------------------------

  describe("Organization schema (graph[0])", () => {
    let org: OrganizationSchema;

    beforeEach(() => {
      graph = buildJsonLdGraph();
      org = graph["@graph"][0];
    });

    it("has @type 'Organization'", () => {
      expect(org["@type"]).toBe("Organization");
    });

    it("name matches SITE_NAME", () => {
      expect(org.name).toBe(SITE_NAME);
    });

    it("url matches SITE_URL", () => {
      expect(org.url).toBe(SITE_URL);
    });

    it("logo matches LOGO_URL", () => {
      expect(org.logo).toBe(LOGO_URL);
    });

    it("description is a non-empty string", () => {
      expect(typeof org.description).toBe("string");
      expect(org.description.trim().length).toBeGreaterThan(0);
    });

    it("sameAs is a non-empty array of absolute URLs", () => {
      expect(Array.isArray(org.sameAs)).toBe(true);
      expect(org.sameAs.length).toBeGreaterThan(0);
      org.sameAs.forEach((url) => {
        expect(typeof url).toBe("string");
        expect(url).toMatch(/^https?:\/\//);
      });
    });

    it("sameAs contains the Twitter profile URL", () => {
      expect(org.sameAs.some((u) => u.includes(TWITTER_HANDLE))).toBe(true);
    });
  });

  // -- WebSite ---------------------------------------------------------------

  describe("WebSite schema (graph[1])", () => {
    let site: WebSiteSchema;

    beforeEach(() => {
      graph = buildJsonLdGraph();
      site = graph["@graph"][1];
    });

    it("has @type 'WebSite'", () => {
      expect(site["@type"]).toBe("WebSite");
    });

    it("name matches SITE_NAME", () => {
      expect(site.name).toBe(SITE_NAME);
    });

    it("url matches SITE_URL", () => {
      expect(site.url).toBe(SITE_URL);
    });

    it("description is a non-empty string", () => {
      expect(typeof site.description).toBe("string");
      expect(site.description.trim().length).toBeGreaterThan(0);
    });

    it("potentialAction has @type SearchAction", () => {
      expect(site.potentialAction["@type"]).toBe("SearchAction");
    });

    it("potentialAction.target.urlTemplate contains the search placeholder", () => {
      expect(site.potentialAction.target.urlTemplate).toContain(
        "{search_term_string}"
      );
    });

    it("potentialAction.target.urlTemplate starts with SITE_URL", () => {
      expect(
        site.potentialAction.target.urlTemplate.startsWith(SITE_URL)
      ).toBe(true);
    });

    it("query-input is 'required name=search_term_string'", () => {
      expect(site.potentialAction["query-input"]).toBe(
        "required name=search_term_string"
      );
    });
  });

  // -- Serialization ---------------------------------------------------------

  describe("JSON serialization round-trip", () => {
    it("serializes to valid JSON without data loss", () => {
      graph = buildJsonLdGraph();
      const serialized = JSON.stringify(graph);
      const reparsed = JSON.parse(serialized) as JsonLdGraph;
      expect(reparsed["@context"]).toBe(graph["@context"]);
      expect(reparsed["@graph"]).toHaveLength(2);
    });

    it("produces compact JSON (no extra whitespace / newlines)", () => {
      graph = buildJsonLdGraph();
      const serialized = JSON.stringify(graph);
      expect(serialized).not.toContain("\n");
    });
  });

  // -- Idempotency -----------------------------------------------------------

  it("returns equal graphs on successive calls (no shared state)", () => {
    const g1 = buildJsonLdGraph();
    const g2 = buildJsonLdGraph();
    expect(JSON.stringify(g1)).toBe(JSON.stringify(g2));
  });

  it("returned graph objects are independent — mutating one does not affect next call", () => {
    const g1 = buildJsonLdGraph();
    (g1["@graph"][0] as OrganizationSchema).name = "MUTATED";
    const g2 = buildJsonLdGraph();
    expect(g2["@graph"][0].name).toBe(SITE_NAME);
  });
});

// ---------------------------------------------------------------------------
// JsonLd component
// ---------------------------------------------------------------------------

describe("<JsonLd> component", () => {
  it("renders a <script> element", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    expect(container.querySelector("script")).not.toBeNull();
  });

  it("script has type='application/ld+json'", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    expect(container.querySelector("script")?.getAttribute("type")).toBe(
      "application/ld+json"
    );
  });

  it("script inner text is valid JSON", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    const text = container.querySelector("script")?.textContent ?? "";
    expect(() => JSON.parse(text)).not.toThrow();
  });

  it("parsed script content has @context 'https://schema.org'", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    const parsed = JSON.parse(
      container.querySelector("script")?.textContent ?? "{}"
    ) as JsonLdGraph;
    expect(parsed["@context"]).toBe("https://schema.org");
  });

  it("parsed script content has @graph with Organization and WebSite", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    const parsed = JSON.parse(
      container.querySelector("script")?.textContent ?? "{}"
    ) as JsonLdGraph;
    expect(parsed["@graph"]).toHaveLength(2);
    expect(parsed["@graph"][0]["@type"]).toBe("Organization");
    expect(parsed["@graph"][1]["@type"]).toBe("WebSite");
  });

  it("rendered JSON contains @context exactly once (no duplicates)", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    const text = container.querySelector("script")?.textContent ?? "";
    const occurrences = (text.match(/"@context"/g) ?? []).length;
    expect(occurrences).toBe(1);
  });

  it("renders exactly one <script> tag (no duplicates)", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    expect(container.querySelectorAll("script")).toHaveLength(1);
  });

  it("organization name in rendered output matches SITE_NAME", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    const parsed = JSON.parse(
      container.querySelector("script")?.textContent ?? "{}"
    ) as JsonLdGraph;
    expect(parsed["@graph"][0].name).toBe(SITE_NAME);
  });

  it("website url in rendered output matches SITE_URL", () => {
    const { container } = render(<JsonLd graph={buildJsonLdGraph()} />);
    const parsed = JSON.parse(
      container.querySelector("script")?.textContent ?? "{}"
    ) as JsonLdGraph;
    expect(parsed["@graph"][1].url).toBe(SITE_URL);
  });
});
