import type { ReactNode } from "react";

import { AppShell } from "@/components/AppShell";
import { AuthProvider } from "@/lib/auth";

export const metadata = {
  title: "Peer Network",
  description: "An ad-and-creativity-funded social platform that pays its users.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>
        <AuthProvider>
          <AppShell>{children}</AppShell>
        </AuthProvider>
      </body>
    </html>
  );
}
