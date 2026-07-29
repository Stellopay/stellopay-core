/**
 * layout.tsx — Root layout for the Stellopay Next.js application.
 *
 * Responsibilities:
 * - Import app/globals.css (CSS custom property tokens for the whole app)
 * - Export Next.js Metadata object (consumed by the framework for <head> tags)
 * - Wrap all pages in shared HTML shell
 *
 * All metadata string values are sourced from metadata-constants.ts so that
 * layout.tsx and page.tsx never diverge from the JSON-LD payload.
 *
 * Accessibility:
 * - lang="en" on <html> satisfies WCAG 2.1 SC 3.1.1 (Language of Page)
 * - <body> has no role; presentational container only
 */

import "./globals.css";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import {
  SITE_URL,
  SITE_NAME,
  SITE_DESCRIPTION,
  LOGO_URL,
  TWITTER_HANDLE,
} from "./metadata-constants";

/**
 * Next.js Metadata export.
 * The framework reads this at build-time and injects the appropriate
 * <meta>, <link>, and <title> tags into the generated HTML.
 */
export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: SITE_NAME,
    template: `%s | ${SITE_NAME}`,
  },
  description: SITE_DESCRIPTION,
  openGraph: {
    type: "website",
    url: SITE_URL,
    siteName: SITE_NAME,
    title: SITE_NAME,
    description: SITE_DESCRIPTION,
    images: [
      {
        url: LOGO_URL,
        width: 1200,
        height: 630,
        alt: `${SITE_NAME} logo`,
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    site: `@${TWITTER_HANDLE}`,
    creator: `@${TWITTER_HANDLE}`,
    title: SITE_NAME,
    description: SITE_DESCRIPTION,
    images: [LOGO_URL],
  },
  icons: {
    icon: "/favicon.ico",
    apple: "/apple-touch-icon.png",
  },
  robots: {
    index: true,
    follow: true,
  },
};

interface RootLayoutProps {
  children: ReactNode;
}

/**
 * Root layout wraps every page. globals.css is imported here so design
 * tokens are available to every component in the tree.
 */
export default function RootLayout({ children }: RootLayoutProps) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
