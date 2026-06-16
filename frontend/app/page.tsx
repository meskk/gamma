// The whole point of the monorepo: these types are GENERATED from the Rust API
// (`backend/bindings`, via ts-rs). Importing them here means a backend API shape
// change makes the frontend fail to TYPECHECK — the contract is enforced at
// compile time, not at runtime. (Type-only imports are erased, so nothing from
// the backend is bundled.)
import type { GemBalance } from "@contract/GemBalance";
import type { Post } from "@contract/Post";

// A compile-time-checked example against the contract. `i64`/`u64` map to bigint.
const demoBalance: GemBalance = { user_id: 1n, balance: 0n };

const demoPosts: Post[] = [];

export default function Home() {
  return (
    <main style={{ fontFamily: "system-ui, sans-serif", padding: "2rem", maxWidth: 640 }}>
      <h1>Peer Network</h1>
      <p>
        Frontend starter, typed against the backend contract in{" "}
        <code>backend/bindings</code>. This is the running foundation — the actual
        UX is built on top from here.
      </p>
      <p>
        Demo (typechecked against <code>GemBalance</code>): user{" "}
        {String(demoBalance.user_id)} has {String(demoBalance.balance)} gems.
      </p>
      <p>{demoPosts.length} posts loaded.</p>
    </main>
  );
}
