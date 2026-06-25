"use client";

import Link from "next/link";

import { useAuth } from "@/lib/auth";

export default function Home() {
  const { token, ready } = useAuth();

  return (
    <div>
      <h1>Peer Network</h1>
      <p>An ad-and-creativity-funded social platform that pays its users.</p>
      {ready &&
        (token ? (
          <p>
            <Link href="/feed">Go to your feed →</Link>
          </p>
        ) : (
          <p>
            <Link href="/login">Log in</Link> or{" "}
            <Link href="/register">create an account</Link> to get started.
          </p>
        ))}
    </div>
  );
}
