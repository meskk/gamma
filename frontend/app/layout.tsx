import type { ReactNode } from "react";

export const metadata = {
  title: "Peer Network",
  description: "An ad-and-creativity-funded social platform that pays its users.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
