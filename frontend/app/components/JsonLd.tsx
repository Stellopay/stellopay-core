/**
 * JsonLd.tsx — React Server Component for JSON-LD structured data.
 */
import type { JsonLdGraph } from "../metadata-constants";

interface JsonLdProps {
  graph: JsonLdGraph;
}

export default function JsonLd({ graph }: JsonLdProps) {
  const json = JSON.stringify(graph);
  return (
    <script
      type="application/ld+json"
      // eslint-disable-next-line react/no-danger
      dangerouslySetInnerHTML={{ __html: json }}
    />
  );
}
