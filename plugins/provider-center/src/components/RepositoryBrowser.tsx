import type {
  ErrorEnvelope,
  ProviderAccountSummary,
  ProviderRepositoryQuery,
  ProviderVisibility,
  RemoteRepository
} from "@git-ramus/contracts";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
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
  const [cloningRepositoryIds, setCloningRepositoryIds] = useState<ReadonlySet<string>>(
    () => new Set()
  );
  const [error, setError] = useState<ErrorEnvelope | null>(null);
  const [rateLimit, setRateLimit] = useState<{
    remaining: number | null;
    retryAfterMs: number | null;
  } | null>(null);
  const generation = useRef(0);
  const initialLoad = useRef(true);
  const loadMoreController = useRef<AbortController | null>(null);
  const cloneGeneration = useRef(0);
  const cloneAccountId = useRef<string | null>(account?.id ?? null);
  const cloningRepositoryIdsRef = useRef(new Set<string>());

  useLayoutEffect(() => {
    const currentGeneration = ++cloneGeneration.current;
    cloneAccountId.current = account?.id ?? null;
    cloningRepositoryIdsRef.current = new Set();
    void Promise.resolve().then(() => {
      if (currentGeneration === cloneGeneration.current) setCloningRepositoryIds(new Set());
    });
    return () => {
      if (currentGeneration === cloneGeneration.current) cloneGeneration.current += 1;
    };
  }, [account?.id, api]);

  useEffect(() => {
    const currentGeneration = ++generation.current;
    loadMoreController.current?.abort();
    loadMoreController.current = null;
    void Promise.resolve().then(() => {
      setItems([]);
      setCursor(null);
      setHasMore(false);
      setError(null);
      setRateLimit(null);
      setLoading(false);
      setLoadingMore(false);
    });
    if (account === null) return () => undefined;
    const controller = new AbortController();
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
      loadMoreController.current?.abort();
      loadMoreController.current = null;
      if (generation.current === currentGeneration) generation.current += 1;
    };
  }, [account, api, query]);

  const loadMore = () => {
    if (account === null || cursor === null || loadingMore) return;
    loadMoreController.current?.abort();
    const controller = new AbortController();
    loadMoreController.current = controller;
    const currentGeneration = generation.current;
    setLoadingMore(true);
    void api
      .listRepositories({ accountId: account.id, query, cursor }, controller.signal)
      .then((page) => {
        if (currentGeneration !== generation.current || loadMoreController.current !== controller)
          return;
        setItems((current) => uniqueRepositories([...current, ...page.items]));
        setCursor(page.nextCursor);
        setHasMore(page.hasMore);
        setRateLimit(page.rateLimit);
      })
      .catch((cause) => {
        if (
          currentGeneration === generation.current &&
          loadMoreController.current === controller &&
          !isAbortError(cause)
        ) {
          setError(normalizeError(cause, "Unable to load more Provider repositories"));
        }
      })
      .finally(() => {
        if (currentGeneration === generation.current && loadMoreController.current === controller) {
          setLoadingMore(false);
          if (loadMoreController.current === controller) loadMoreController.current = null;
        }
      });
  };

  const createCloneIntent = (repository: RemoteRepository) => {
    if (
      account === null ||
      !canCloneRepository(repository) ||
      cloningRepositoryIdsRef.current.has(repository.repositoryId)
    ) {
      return;
    }
    const accountId = account.id;
    const currentGeneration = cloneGeneration.current;
    cloningRepositoryIdsRef.current.add(repository.repositoryId);
    setCloningRepositoryIds(new Set(cloningRepositoryIdsRef.current));
    setError(null);
    void api
      .createCloneIntent(accountId, repository.repositoryId)
      .then((reference) => {
        if (currentGeneration !== cloneGeneration.current || cloneAccountId.current !== accountId) {
          return undefined;
        }
        return api.openCloneIntent(reference.intentId);
      })
      .catch((cause) => {
        if (currentGeneration === cloneGeneration.current && cloneAccountId.current === accountId) {
          setError(normalizeError(cause, "Unable to create Clone intent"));
        }
      })
      .finally(() => {
        if (currentGeneration !== cloneGeneration.current || cloneAccountId.current !== accountId) {
          return;
        }
        cloningRepositoryIdsRef.current.delete(repository.repositoryId);
        setCloningRepositoryIds(new Set(cloningRepositoryIdsRef.current));
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
            maxLength={256}
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
            maxLength={1024}
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
            <div className="repository-actions">
              <small className="repository-url">{item.webUrl}</small>
              <button
                type="button"
                aria-label={`Clone ${item.fullName}`}
                title={cloneDisabledReason(item)}
                disabled={!canCloneRepository(item) || cloningRepositoryIds.has(item.repositoryId)}
                onClick={() => createCloneIntent(item)}
              >
                {cloningRepositoryIds.has(item.repositoryId) ? "Opening…" : "Clone"}
              </button>
            </div>
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

function canCloneRepository(repository: RemoteRepository): boolean {
  return (
    !repository.archived &&
    (["read", "write", "admin"] as readonly string[]).includes(repository.permission)
  );
}

function cloneDisabledReason(repository: RemoteRepository): string | undefined {
  if (repository.archived) return "Archived repositories cannot be cloned";
  if (!canCloneRepository(repository)) return "Read access is required to clone this repository";
  return undefined;
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}
