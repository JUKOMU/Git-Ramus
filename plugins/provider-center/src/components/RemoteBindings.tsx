import type {
  ErrorEnvelope,
  ProviderAccountSummary,
  ProviderBindingSuggestion,
  ProviderInstance,
  ProviderBinding
} from "@git-ramus/contracts";
import { useEffect, useRef, useState } from "react";
import type { ProviderCenterApi } from "../api";
import { normalizeError } from "../api";
import { ErrorNotice } from "./InstancePanel";

interface RemoteBindingsProps {
  api: ProviderCenterApi;
  instance: ProviderInstance | null;
  account: ProviderAccountSummary | null;
  accounts: ProviderAccountSummary[];
}

export function RemoteBindings({ api, instance, account, accounts }: RemoteBindingsProps) {
  const [suggestions, setSuggestions] = useState<ProviderBindingSuggestion[]>([]);
  const [bindings, setBindings] = useState<ProviderBinding[]>([]);
  const [selectedCandidates, setSelectedCandidates] = useState<Record<string, string>>({});
  const [bindingAccounts, setBindingAccounts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorEnvelope | null>(null);
  const scanGeneration = useRef(0);
  const bindingGeneration = useRef(0);
  const scanController = useRef<AbortController | null>(null);

  useEffect(() => {
    const generation = ++bindingGeneration.current;
    scanGeneration.current += 1;
    scanController.current?.abort();
    scanController.current = null;
    void Promise.resolve().then(() => {
      setBindings([]);
      setSuggestions([]);
      setSelectedCandidates({});
      setBindingAccounts({});
      setBusy(false);
      setError(null);
    });
    if (instance === null || account === null || account.instanceId !== instance.id)
      return () => undefined;
    void api
      .listBindings(account.id)
      .then((response) => {
        if (generation === bindingGeneration.current) setBindings(response.items);
      })
      .catch((cause) => {
        if (generation === bindingGeneration.current) {
          setError(normalizeError(cause, "Unable to load remote bindings"));
        }
      });
    return () => {
      if (bindingGeneration.current === generation) bindingGeneration.current += 1;
      scanGeneration.current += 1;
      scanController.current?.abort();
      scanController.current = null;
    };
  }, [account, api, instance]);

  const scan = () => {
    if (instance === null || account === null) return;
    scanController.current?.abort();
    const generation = ++scanGeneration.current;
    const controller = new AbortController();
    scanController.current = controller;
    setBusy(true);
    setError(null);
    setSuggestions([]);
    void api
      .matchLocalRemotes({ instanceId: instance.id, accountId: account.id }, controller.signal)
      .then((response) => {
        if (generation === scanGeneration.current) setSuggestions(response.items);
      })
      .catch((cause) => {
        if (
          generation === scanGeneration.current &&
          !(cause instanceof DOMException && cause.name === "AbortError")
        )
          setError(normalizeError(cause, "Unable to match local remotes"));
      })
      .finally(() => {
        if (generation === scanGeneration.current) {
          setBusy(false);
          if (scanController.current === controller) scanController.current = null;
        }
      });
  };

  const bind = async (suggestion: ProviderBindingSuggestion) => {
    if (instance === null || account === null || !suggestions.includes(suggestion)) return;
    const generation = scanGeneration.current;
    const providerRepositoryId =
      suggestion.status === "ambiguous"
        ? selectedCandidates[suggestionKey(suggestion)]
        : suggestion.providerRepositoryId;
    if (
      providerRepositoryId === undefined ||
      providerRepositoryId === null ||
      providerRepositoryId.length === 0
    )
      return;
    setBusy(true);
    setError(null);
    try {
      await api.bindRemote({
        repositoryId: suggestion.repositoryId,
        remoteName: suggestion.remoteName,
        instanceId: instance.id,
        accountId: bindingAccounts[suggestionKey(suggestion)] || null,
        providerRepositoryId
      });
      const response = await api.listBindings(account.id);
      if (generation !== scanGeneration.current) return;
      setBindings(response.items);
      setSuggestions((current) =>
        current.map((item) =>
          item.repositoryId === suggestion.repositoryId && item.remoteName === suggestion.remoteName
            ? { ...item, status: "suggested" }
            : item
        )
      );
    } catch (cause) {
      if (generation === scanGeneration.current) {
        setError(normalizeError(cause, "Unable to bind the local remote"));
      }
    } finally {
      if (generation === scanGeneration.current) setBusy(false);
    }
  };

  return (
    <section className="panel remote-bindings" aria-labelledby="remote-bindings-heading">
      <header className="panel-heading">
        <div>
          <p className="eyebrow">Local Git</p>
          <h2 id="remote-bindings-heading">Remote bindings</h2>
        </div>
        <button
          type="button"
          disabled={busy || instance === null || account === null}
          onClick={scan}
        >
          Scan local remotes
        </button>
      </header>
      {error === null ? null : <ErrorNotice error={error} />}
      {account === null ? (
        <p className="muted">Select an account to inspect local remotes.</p>
      ) : null}
      <ul className="binding-suggestions" aria-label="Remote binding suggestions">
        {suggestions.length === 0 ? <li className="muted">No scan results yet.</li> : null}
        {suggestions.map((suggestion) => {
          const key = suggestionKey(suggestion);
          const candidate =
            suggestion.status === "ambiguous"
              ? selectedCandidates[key]
              : suggestion.providerRepositoryId;
          return (
            <li key={key} className="binding-suggestion">
              <div>
                <strong>{suggestion.remoteName}</strong>
                <small>
                  {suggestion.repositoryId} · {suggestion.status}
                </small>
              </div>
              {suggestion.status === "ambiguous" ? (
                <label>
                  Choose a repository
                  <select
                    value={selectedCandidates[key] ?? ""}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setSelectedCandidates((current) => ({ ...current, [key]: value }));
                    }}
                  >
                    <option value="">Choose candidate</option>
                    {suggestion.candidates.map((item) => (
                      <option value={item.repositoryId} key={item.repositoryId}>
                        {item.fullName}
                      </option>
                    ))}
                  </select>
                </label>
              ) : null}
              {suggestion.status === "suggested" || suggestion.status === "ambiguous" ? (
                <>
                  <label>
                    Binding account
                    <select
                      value={bindingAccounts[key] ?? ""}
                      onChange={(event) => {
                        const value = event.currentTarget.value;
                        setBindingAccounts((current) => ({ ...current, [key]: value }));
                      }}
                    >
                      <option value="">Use instance default</option>
                      {accounts.map((item) => (
                        <option value={item.id} key={item.id}>
                          {item.displayName ?? item.username}
                        </option>
                      ))}
                    </select>
                  </label>
                  <button
                    type="button"
                    disabled={busy || candidate === undefined || candidate === ""}
                    onClick={() => void bind(suggestion)}
                  >
                    Bind
                  </button>
                </>
              ) : (
                <span className="muted">No verified Provider match.</span>
              )}
            </li>
          );
        })}
      </ul>
      <h3>Current bindings</h3>
      <ul className="current-bindings" aria-label="Current remote bindings">
        {bindings.length === 0 ? <li className="muted">No bindings for this account.</li> : null}
        {bindings.map((binding) => (
          <li key={`${binding.repositoryId}:${binding.remoteName}`}>
            <span>
              <strong>{binding.remoteName}</strong>
              <small>
                {binding.fullName} · {binding.bindingSource}
              </small>
            </span>
            <button
              type="button"
              disabled={busy}
              onClick={() =>
                void (async () => {
                  const generation = bindingGeneration.current;
                  setBusy(true);
                  try {
                    await api.unbindRemote(binding.repositoryId, binding.remoteName);
                    if (generation === bindingGeneration.current) {
                      setBindings((current) => current.filter((item) => item !== binding));
                    }
                  } catch (cause) {
                    if (generation === bindingGeneration.current) {
                      setError(normalizeError(cause, "Unable to remove the remote binding"));
                    }
                  } finally {
                    if (generation === bindingGeneration.current) setBusy(false);
                  }
                })()
              }
            >
              Unbind
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function suggestionKey(suggestion: ProviderBindingSuggestion): string {
  return `${suggestion.repositoryId}:${suggestion.remoteName}`;
}
