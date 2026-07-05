// The ONE data-fetching hook (FE-B1): loading/error state plus the stale-guard
// every page used to hand-roll (`let stale = false` per effect). A generation
// counter drops out-of-order responses, deps changes reset the state (no stale
// flash on client-side navigation), `reload` refetches in place, and
// `enabled: false` idles without fetching (replaces `if (!token) return`
// guards). The error is exposed RAW — pages keep mapping ApiError codes to
// their own copy. Deliberately not react-query: a handful of call sites, no
// cross-page cache needs, zero-deps line (see MASTERPLAN §6.2).

import { useCallback, useEffect, useRef, useState, type DependencyList } from "react";

export function useFetch<T>(
  fetcher: () => Promise<T>,
  deps: DependencyList,
  opts: { enabled?: boolean } = {},
) {
  const enabled = opts.enabled !== false;
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(enabled);
  const [tick, setTick] = useState(0);
  const gen = useRef(0);

  // The fetcher is intentionally NOT a dependency (pages pass inline closures);
  // the caller-supplied deps decide when to refetch, exactly like before.
  /* eslint-disable react-hooks/exhaustive-deps */
  useEffect(() => {
    const g = ++gen.current;
    setData(null);
    setError(null);
    if (!enabled) {
      setLoading(false);
      return;
    }
    setLoading(true);
    fetcher()
      .then((d) => {
        if (g !== gen.current) return;
        setData(d);
        setLoading(false);
      })
      .catch((e: unknown) => {
        if (g !== gen.current) return;
        setError(e);
        setLoading(false);
      });
  }, [...deps, enabled, tick]);
  /* eslint-enable react-hooks/exhaustive-deps */

  const reload = useCallback(() => setTick((t) => t + 1), []);

  return { data, error, loading, reload };
}
