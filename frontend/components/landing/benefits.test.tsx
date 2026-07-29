/**
 * benefits.test.tsx
 *
 * Full test coverage for components/landing/benefits.tsx
 *
 * Coverage goals
 * ──────────────
 * - Default render: section, heading, subtitle, card list present
 * - Each default card: title, description, icon badge rendered
 * - Token usage: no raw hex values appear anywhere in inline styles
 * - Custom items prop: overrides default data
 * - Empty state: shows empty-state message, no card list
 * - Single item: renders correctly without grid issues
 * - Long text: does not break layout (snapshot/structure)
 * - Accessibility: landmark roles, aria-labelledby, aria-hidden on icons
 * - Dark mode: CSS custom properties referenced (tokens, not hex)
 * - RTL: dir="rtl" on a wrapper doesn't break structure
 * - Data-testid hooks are present for all key elements
 *
 * Run: cd frontend && npm test
 */

import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import Benefits, {
  DEFAULT_BENEFITS,
  type Benefit,
  type BenefitsProps,
} from "../../components/landing/benefits";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function renderBenefits(props: BenefitsProps = {}) {
  return render(<Benefits {...props} />);
}

/** Extract the inline style string of an element as lowercase for comparisons. */
function inlineStyle(el: HTMLElement): string {
  return el.getAttribute("style") ?? "";
}

// ---------------------------------------------------------------------------
// Default render
// ---------------------------------------------------------------------------

describe("Benefits — default render", () => {
  it("renders the section landmark", () => {
    renderBenefits();
    expect(screen.getByTestId("benefits-section").tagName).toBe("SECTION");
  });

  it("section has aria-labelledby pointing to the heading", () => {
    renderBenefits();
    const section = screen.getByTestId("benefits-section");
    const labelId = section.getAttribute("aria-labelledby");
    expect(labelId).toBeTruthy();
    const heading = document.getElementById(labelId!);
    expect(heading).not.toBeNull();
    expect(heading?.tagName).toBe("H2");
  });

  it("renders the heading text", () => {
    renderBenefits();
    expect(screen.getByTestId("benefits-heading")).toHaveTextContent(
      "Why Stellopay?"
    );
  });

  it("renders the subtitle text", () => {
    renderBenefits();
    const subtitle = screen.getByTestId("benefits-subtitle");
    expect(subtitle.textContent?.trim().length).toBeGreaterThan(0);
  });

  it("renders a <ul role='list'> for the cards", () => {
    renderBenefits();
    const list = screen.getByTestId("benefits-list");
    expect(list.tagName).toBe("UL");
    expect(list).toHaveAttribute("role", "list");
  });

  it("renders exactly 4 default benefit cards", () => {
    renderBenefits();
    expect(screen.getAllByTestId("benefit-card")).toHaveLength(4);
  });
});

// ---------------------------------------------------------------------------
// Default benefit data
// ---------------------------------------------------------------------------

describe("DEFAULT_BENEFITS constant", () => {
  it("has exactly 4 items", () => {
    expect(DEFAULT_BENEFITS).toHaveLength(4);
  });

  it("every item has non-empty title, description, icon, gradient, iconBg", () => {
    DEFAULT_BENEFITS.forEach((b) => {
      expect(b.title.trim().length).toBeGreaterThan(0);
      expect(b.description.trim().length).toBeGreaterThan(0);
      expect(b.icon.trim().length).toBeGreaterThan(0);
      expect(b.gradient.trim().length).toBeGreaterThan(0);
      expect(b.iconBg.trim().length).toBeGreaterThan(0);
    });
  });

  it("every gradient references a CSS custom property (var(--...))", () => {
    DEFAULT_BENEFITS.forEach((b) => {
      expect(b.gradient).toMatch(/^var\(--/);
    });
  });

  it("every iconBg references a CSS custom property (var(--...))", () => {
    DEFAULT_BENEFITS.forEach((b) => {
      expect(b.iconBg).toMatch(/^var\(--/);
    });
  });

  it("titles include expected keywords", () => {
    const titles = DEFAULT_BENEFITS.map((b) => b.title.toLowerCase());
    expect(titles.some((t) => t.includes("escrow") || t.includes("payroll"))).toBe(true);
    expect(titles.some((t) => t.includes("currency"))).toBe(true);
    expect(titles.some((t) => t.includes("governance"))).toBe(true);
    expect(titles.some((t) => t.includes("compliance"))).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Individual card structure
// ---------------------------------------------------------------------------

describe("BenefitCard structure", () => {
  beforeEach(() => {
    renderBenefits();
  });

  it("each card renders title and description", () => {
    const cards = screen.getAllByTestId("benefit-card");
    DEFAULT_BENEFITS.forEach((benefit, i) => {
      const card = cards[i];
      expect(within(card).getByTestId("benefit-title")).toHaveTextContent(
        benefit.title
      );
      expect(
        within(card).getByTestId("benefit-description")
      ).toHaveTextContent(benefit.description);
    });
  });

  it("each card renders an icon badge with aria-hidden", () => {
    const badges = screen.getAllByTestId("benefit-icon-badge");
    expect(badges).toHaveLength(DEFAULT_BENEFITS.length);
    badges.forEach((badge) => {
      expect(badge).toHaveAttribute("aria-hidden", "true");
    });
  });

  it("icon badges contain the expected emoji characters", () => {
    const badges = screen.getAllByTestId("benefit-icon-badge");
    DEFAULT_BENEFITS.forEach((benefit, i) => {
      expect(badges[i].textContent).toBe(benefit.icon);
    });
  });

  it("card titles are <h3> elements", () => {
    screen.getAllByTestId("benefit-title").forEach((el) => {
      expect(el.tagName).toBe("H3");
    });
  });

  it("card descriptions are <p> elements", () => {
    screen.getAllByTestId("benefit-description").forEach((el) => {
      expect(el.tagName).toBe("P");
    });
  });
});

// ---------------------------------------------------------------------------
// Token usage — no raw hex values in inline styles
// ---------------------------------------------------------------------------

describe("Token usage — no raw hex values in inline styles", () => {
  it("section background uses a CSS custom property, not a hex literal", () => {
    renderBenefits();
    const section = screen.getByTestId("benefits-section");
    const style = inlineStyle(section);
    // Must reference a var(--...) token
    expect(style).toMatch(/var\(--/);
    // Must NOT contain a bare hex colour like #0f172a or #1e1b4b
    expect(style).not.toMatch(/#[0-9a-fA-F]{3,8}/);
  });

  it("icon badge gradient uses a CSS custom property, not a hex literal", () => {
    renderBenefits();
    screen.getAllByTestId("benefit-icon-badge").forEach((badge) => {
      const style = inlineStyle(badge);
      expect(style).toMatch(/var\(--/);
      expect(style).not.toMatch(/#[0-9a-fA-F]{3,8}/);
    });
  });

  it("heading colour uses a CSS custom property", () => {
    renderBenefits();
    const heading = screen.getByTestId("benefits-heading");
    expect(inlineStyle(heading)).toMatch(/var\(--/);
    expect(inlineStyle(heading)).not.toMatch(/#[0-9a-fA-F]{3,8}/);
  });

  it("subtitle colour uses a CSS custom property", () => {
    renderBenefits();
    const subtitle = screen.getByTestId("benefits-subtitle");
    expect(inlineStyle(subtitle)).toMatch(/var\(--/);
    expect(inlineStyle(subtitle)).not.toMatch(/#[0-9a-fA-F]{3,8}/);
  });

  it("card title colour uses a CSS custom property", () => {
    renderBenefits();
    screen.getAllByTestId("benefit-title").forEach((el) => {
      expect(inlineStyle(el)).toMatch(/var\(--/);
      expect(inlineStyle(el)).not.toMatch(/#[0-9a-fA-F]{3,8}/);
    });
  });

  it("card description colour uses a CSS custom property", () => {
    renderBenefits();
    screen.getAllByTestId("benefit-description").forEach((el) => {
      expect(inlineStyle(el)).toMatch(/var\(--/);
      expect(inlineStyle(el)).not.toMatch(/#[0-9a-fA-F]{3,8}/);
    });
  });
});

// ---------------------------------------------------------------------------
// Custom items prop
// ---------------------------------------------------------------------------

describe("Benefits — custom items prop", () => {
  const customItems: Benefit[] = [
    {
      title: "Custom Feature A",
      description: "Description for feature A that is long enough to test wrapping.",
      icon: "🚀",
      gradient: "var(--gradient-card-indigo)",
      iconBg: "var(--color-icon-bg-indigo)",
    },
    {
      title: "Custom Feature B",
      description: "Description for feature B.",
      icon: "🌍",
      gradient: "var(--gradient-card-violet)",
      iconBg: "var(--color-icon-bg-violet)",
    },
  ];

  it("renders exactly the provided items, not the defaults", () => {
    renderBenefits({ items: customItems });
    expect(screen.getAllByTestId("benefit-card")).toHaveLength(2);
    expect(screen.getByText("Custom Feature A")).toBeInTheDocument();
    expect(screen.getByText("Custom Feature B")).toBeInTheDocument();
    // Default items should not be present
    expect(screen.queryByText("Payroll escrow")).not.toBeInTheDocument();
  });

  it("icon badges reflect custom emoji", () => {
    renderBenefits({ items: customItems });
    const badges = screen.getAllByTestId("benefit-icon-badge");
    expect(badges[0].textContent).toBe("🚀");
    expect(badges[1].textContent).toBe("🌍");
  });
});

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

describe("Benefits — empty state", () => {
  it("shows empty-state message when items is empty array", () => {
    renderBenefits({ items: [] });
    expect(screen.getByTestId("benefits-empty")).toBeInTheDocument();
    expect(screen.getByTestId("benefits-empty")).toHaveTextContent(
      "No benefits to display."
    );
  });

  it("empty-state element has role='status' for AT announcement", () => {
    renderBenefits({ items: [] });
    expect(screen.getByTestId("benefits-empty")).toHaveAttribute(
      "role",
      "status"
    );
  });

  it("does NOT render the card list when items is empty", () => {
    renderBenefits({ items: [] });
    expect(screen.queryByTestId("benefits-list")).not.toBeInTheDocument();
  });

  it("does NOT render any cards when items is empty", () => {
    renderBenefits({ items: [] });
    expect(screen.queryAllByTestId("benefit-card")).toHaveLength(0);
  });

  it("still renders heading and subtitle in empty state", () => {
    renderBenefits({ items: [] });
    expect(screen.getByTestId("benefits-heading")).toBeInTheDocument();
    expect(screen.getByTestId("benefits-subtitle")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Single item
// ---------------------------------------------------------------------------

describe("Benefits — single item", () => {
  const oneItem: Benefit[] = [
    {
      title: "Solo Feature",
      description: "Only one card.",
      icon: "⭐",
      gradient: "var(--gradient-card-emerald)",
      iconBg: "var(--color-icon-bg-emerald)",
    },
  ];

  it("renders exactly one card", () => {
    renderBenefits({ items: oneItem });
    expect(screen.getAllByTestId("benefit-card")).toHaveLength(1);
  });

  it("renders the list (not empty state) for a single item", () => {
    renderBenefits({ items: oneItem });
    expect(screen.getByTestId("benefits-list")).toBeInTheDocument();
    expect(screen.queryByTestId("benefits-empty")).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Long text / edge cases
// ---------------------------------------------------------------------------

describe("Benefits — long text edge cases", () => {
  it("renders very long title without crashing", () => {
    const longTitle =
      "This is an extremely long benefit title that exceeds normal display widths and should wrap gracefully without breaking the layout or causing any overflow issues";
    renderBenefits({
      items: [
        {
          title: longTitle,
          description: "Short description.",
          icon: "📝",
          gradient: "var(--gradient-card-cyan)",
          iconBg: "var(--color-icon-bg-cyan)",
        },
      ],
    });
    expect(screen.getByText(longTitle)).toBeInTheDocument();
  });

  it("renders very long description without crashing", () => {
    const longDesc = "A ".repeat(200) + "end.";
    renderBenefits({
      items: [
        {
          title: "Short title",
          description: longDesc,
          icon: "📊",
          gradient: "var(--gradient-card-indigo)",
          iconBg: "var(--color-icon-bg-indigo)",
        },
      ],
    });
    expect(screen.getByTestId("benefit-description")).toBeInTheDocument();
  });

  it("renders special characters in title and description", () => {
    renderBenefits({
      items: [
        {
          title: "Title with <special> & \"chars\"",
          description: "Desc with 日本語 & emoji 🎉",
          icon: "🔒",
          gradient: "var(--gradient-card-violet)",
          iconBg: "var(--color-icon-bg-violet)",
        },
      ],
    });
    expect(
      screen.getByText('Title with <special> & "chars"')
    ).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Accessibility
// ---------------------------------------------------------------------------

describe("Benefits — accessibility", () => {
  it("section has accessible name via aria-labelledby", () => {
    renderBenefits();
    const section = screen.getByTestId("benefits-section");
    expect(section).toHaveAttribute("aria-labelledby");
  });

  it("heading id matches section aria-labelledby", () => {
    renderBenefits();
    const section = screen.getByTestId("benefits-section");
    const headingId = section.getAttribute("aria-labelledby")!;
    const heading = document.getElementById(headingId);
    expect(heading).not.toBeNull();
    expect(heading?.tagName).toBe("H2");
  });

  it("icon badges are hidden from assistive technology", () => {
    renderBenefits();
    screen.getAllByTestId("benefit-icon-badge").forEach((badge) => {
      expect(badge).toHaveAttribute("aria-hidden", "true");
    });
  });

  it("card list has role='list' for correct AT semantics", () => {
    renderBenefits();
    expect(screen.getByTestId("benefits-list")).toHaveAttribute("role", "list");
  });

  it("each card is wrapped in a <li> inside the list", () => {
    renderBenefits();
    const list = screen.getByTestId("benefits-list");
    const listItems = within(list).getAllByRole("listitem");
    expect(listItems).toHaveLength(DEFAULT_BENEFITS.length);
  });

  it("each card is an <article> element", () => {
    renderBenefits();
    screen.getAllByTestId("benefit-card").forEach((card) => {
      expect(card.tagName).toBe("ARTICLE");
    });
  });
});

// ---------------------------------------------------------------------------
// RTL layout
// ---------------------------------------------------------------------------

describe("Benefits — RTL layout", () => {
  it("renders without error when wrapped in a dir=rtl container", () => {
    const { container } = render(
      <div dir="rtl">
        <Benefits />
      </div>
    );
    expect(container.querySelector("[data-testid='benefits-section']")).not.toBeNull();
    expect(container.querySelectorAll("[data-testid='benefit-card']")).toHaveLength(
      DEFAULT_BENEFITS.length
    );
  });
});

// ---------------------------------------------------------------------------
// Dark mode — tokens are referenced (CSS vars are honoured by the browser;
// in JSDOM we verify the attribute strings contain var(--) references)
// ---------------------------------------------------------------------------

describe("Benefits — dark mode token references", () => {
  it("section background token is present in inline style attribute", () => {
    renderBenefits();
    const section = screen.getByTestId("benefits-section");
    expect(inlineStyle(section)).toContain("var(--gradient-section-bg)");
  });

  it("icon badge gradient token is present in data-gradient attribute", () => {
    renderBenefits();
    const badges = screen.getAllByTestId("benefit-icon-badge");
    DEFAULT_BENEFITS.forEach((benefit, i) => {
      // The component sets data-gradient so tests can assert the token
      // without relying on JSDOM's CSS shorthand serialisation order.
      expect(badges[i]).toHaveAttribute("data-gradient", benefit.gradient);
    });
  });
});
