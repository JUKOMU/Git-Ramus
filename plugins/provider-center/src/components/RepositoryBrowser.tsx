import type {
  ErrorEnvelope,
  ProviderAccountSummary,
  ProviderRepositoryQuery,
  ProviderVisibility,
  RemoteRepository
} from "@git-ramus/contracts";
import { useEffect, useRef, useState } from "react";
import type { ProviderCenterApi } from "../api";
import { normalizeError } from "../api";
import { ErrorNotice } from "./InstancePanel";

interface RepositoryBrowserProps {
  api: ProviderCenterApi;
  account: ProviderAccountSummary | null;
}

const emptyQuery = {
  search: "",
  visibility: null as ProviderVisibility | null,
  namespace: null as string | null,
  archived: "all" as ProviderRepositoryQuery["archived"],
  sort: "name" as ProviderRepositoryQuery["sort"],
  direction: "asc" as ProviderRepositoryQuery["direction"],
  pageSize: 30
};

export function RepositoryBrowser({ api, account }: RepositoryBrowserProps) {
  const [query, setQuery] = useState(emptyQuery);
  const [items, setItems] = useState<RemoteRepository[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<ErrorEnvelope | null>(null);
  const [rateLimit, setRateLimit] = useState<{
    remaining: number | null;
    retryAfterMs: number | null;
  } | null>(null);
  const generation = useRef(0);
  const initialLoad = useRef(true);

  useEffect(() => {
    void Promise.resolve().then(() => {
      setItems([]);
      setCursor(null);
      setHasMore(false);
      setError(null);
      setRateLimit(null);
    });
    if (account === null) return;
    const controller = new AbortController();
    const currentGeneration = ++generation.current;
    const delay = initialLoad.current ? 0 : 120;
    initialLoad.current = false;
    const timer = window.setTimeout(() => {
      setLoading(true);
      void api
        .listRepositories({ accountId: account.id, query, cursor: null }, controller.signal)
        .then((page) => {
          if (currentGeneration !== generation.current) return;
          setItems(uniqueRepositories(page.items));
          setCursor(page.nextCursor);
          setHasMore(page.hasMore);
          setRateLimit(page.rateLimit);
          setError(null);
        })
        .catch((cause) => {
          if (currentGeneration !== generation.current || isAbortError(cause)) return;
          setError(normalizeError(cause, "Unable to load Provider repositories"));
        })
        .finally(() => {
          if (currentGeneration === generation.current) setLoading(false);
        });
    }, delay);
    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [account, api, query]);

  const loadMore = () => {
    if (account === null || cursor === null || loadingMore) return;
    const controller = new AbortController();
    const currentGeneration = generation.current;
    setLoadingMore(true);
    void api
      .listRepositories({ accountId: account.id, query, cursor }, controller.signal)
      .then((page) => {
        if (currentGeneration !== generation.current) return;
        setItems((current) => uniqueRepositories([...current, ...page.items]));
        setCursor(page.nextCursor);
        setHasMore(page.hasMore);
        setRateLimit(page.rateLimit);
      })
      .catch((cause) => {
        if (currentGeneration === generation.current && !isAbortError(cause)) {
          setError(normalizeError(cause, "Unable to load more Provider repositories"));
        }
      })
      .finally(() => {
        if (currentGeneration === generation.current) setLoadingMore(false);
      });
  };

  return (
    <section className="panel repository-browser" aria-labelledby="repository-browser-heading">
      <header className="panel-heading">
        <div>
          <p className="eyebrow">Discovery</p>
          <h2 id="repository-browser-heading">Repository browser</h2>
        </div>
        {loading ? (
          <span className="muted" aria-live="polite">
            Loading…
          </span>
        ) : null}
      </header>
      {account === null ? (
        <p className="muted">Connect and select an account to browse repositories.</p>
      ) : null}
      {rateLimit === null ? null : (
        <div className="rate-limit-banner" role="status">
          {rateLimit.remaining === 0
            ? "Provider rate limit reached."
            : "Provider request budget is low."}
          {rateLimit.retryAfterMs === null ? null : ` Retry after ${rateLimit.retryAfterMs} ms.`}
        </div>
      )}
      {error === null ? null : <ErrorNotice error={error} />}
      <div className="repository-filters" aria-label="Repository filters">
        <label>
          Search repositories
          <input
            type="search"
            role="searchbox"
            value={query.search}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setQuery((current) => ({ ...current, search: value }));
            }}
          />
        </label>
        <label>
          Namespace
          <input
            value={query.namespace ?? ""}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setQuery((current) => ({ ...current, namespace: value === "" ? null : value }));
            }}
          />
        </label>
        <label>
          Visibility
          <select
            value={query.visibility ?? "all"}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setQuery((current) => ({
                ...current,
                visibility: value === "all" ? null : (value as ProviderVisibility)
              }));
            }}
          >
            <option value="all">All visibility</option>
            <option value="public">Public</option>
            <option value="internal">Internal</option>
            <option value="private">Private</option>
          </select>
        </label>
        <label>
          Sort
          <select
            value={query.sort}
            onChange={(event) => {
              const value = event.currentTarget.value as ProviderRepositoryQuery["sort"];
              setQuery((current) => ({ ...current, sort: value }));
            }}
          >
            <option value="name">Name</option>
            <option value="updated">Recently updated</option>
          </select>
        </label>
        <label>
          Direction
          <select
            value={query.direction}
            onChange={(event) => {
              const value = event.currentTarget.value as ProviderRepositoryQuery["direction"];
              setQuery((current) => ({ ...current, direction: value }));
            }}
          >
            <option value="asc">Ascending</option>
            <option value="desc">Descending</option>
          </select>
        </label>
        <label>
          Archived
          <select
            value={query.archived}
            onChange={(event) => {
              const value = event.currentTarget.value as ProviderRepositoryQuery["archived"];
              setQuery((current) => ({ ...current, archived: value }));
            }}
          >
            <option value="all">All repositories</option>
            <option value="active">Active</option>
            <option value="archived">Archived</option>
          </select>
        </label>
      </div>
      <ul className="repository-results" aria-label="Discovered repositories">
        {items.length === 0 && !loading ? (
          <li className="muted">No repositories match the current filters.</li>
        ) : null}
        {items.map((item) => (
          <li key={item.repositoryId}>
            <span>
              <strong>{item.fullName}</strong>
              <small>
                {item.visibility} · {item.permission}
                {item.archived ? " · archived" : ""}
              </small>
            </span>
            <a href={item.webUrl} target="_blank" rel="noreferrer">
              Open
            </a>
          </li>
        ))}
      </ul>
      {hasMore ? (
        <button type="button" onClick={loadMore} disabled={loadingMore}>
          {loadingMore ? "Loading…" : "Load more"}
        </button>
      ) : null}
    </section>
  );
}

function uniqueRepositories(items: RemoteRepository[]): RemoteRepository[] {
  const seen = new Set<string>();
  return items.filter((item) => {
    if (seen.has(item.repositoryId)) return false;
    seen.add(item.repositoryId);
    return true;
  });
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}
