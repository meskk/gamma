import { Hanken_Grotesk, Inter } from "next/font/google";
import type { ReactNode } from "react";

import "./globals.css";
import { AppShell } from "@/components/AppShell";
import { AuthProvider } from "@/lib/auth";

// Self-hosted via next/font (no runtime request to fonts.googleapis.com): GDPR-safe
// and no FOUT. Exposed as CSS variables so any page (e.g. the glass login) can use
// them without a page-scoped @import.
const inter = Inter({
  subsets: ["latin"],
  weight: ["400", "500", "600"],
  variable: "--font-inter",
  display: "swap",
});
const hanken = Hanken_Grotesk({
  subsets: ["latin"],
  weight: ["600", "700"],
  variable: "--font-hanken",
  display: "swap",
});

export const metadata = {
  title: "Peer Network",
  description: "An ad-and-creativity-funded social platform that pays its users.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${inter.variable} ${hanken.variable}`}>
      <body>
        <AuthProvider>
          <AppShell>{children}</AppShell>
        </AuthProvider>
      </body>
    </html>
  );
}
