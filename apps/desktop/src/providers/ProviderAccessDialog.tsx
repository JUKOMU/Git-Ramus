import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type { ProviderAuthorizedAccount } from "@git-ramus/contracts";
import type { ActivePrompt, PromptBroker } from "./promptBroker";
import type { ProviderAccessPromptRequest } from "./promptPorts";

interface ProviderAccessDialogProps {
  broker: PromptBroker<ProviderAccessPromptRequest, string[]>;
}

export function ProviderAccessDialog({ broker }: ProviderAccessDialogProps) {
  const [active, setActive] = useState<ActivePrompt<ProviderAccessPromptRequest> | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const activeRef = useRef<ActivePrompt<ProviderAccessPromptRequest> | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const unsubscribe = broker.subscribe((next) => {
      activeRef.current = next;
      setActive(next);
      setSelected(new Set());
    });
    return () => {
      const current = activeRef.current;
      activeRef.current = null;
      unsubscribe();
      setSelected(new Set());
      if (current !== null) broker.cancel(current.id);
    };
  }, [broker]);

  useEffect(() => {
    if (active === null) {
      previousFocusRef.current?.focus();
      previousFocusRef.current = null;
      return undefined;
    }
    previousFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const firstControl = dialogRef.current?.querySelector<HTMLElement>("input, button");
    firstControl?.focus();
    return () => {
      if (activeRef.current === null) previousFocusRef.current?.focus();
    };
  }, [active]);

  if (active === null) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      broker.cancel(active.id);
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = focusableElements(dialogRef.current);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  const toggleAccount = (accountId: string, checked: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      if (checked) next.add(accountId);
      else next.delete(accountId);
      return next;
    });
  };

  return (
    <div className="provider-prompt-overlay" role="presentation">
      <div
        ref={dialogRef}
        className="provider-prompt-dialog provider-access-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="provider-access-title"
        aria-describedby="provider-access-guidance"
        data-testid="provider-access-dialog"
        onKeyDown={onKeyDown}
      >
        <h2 id="provider-access-title">Provider access</h2>
        <p id="provider-access-guidance">
          <strong>{active.request.pluginId}</strong> is requesting read-only access to selected
          Provider accounts. Only the accounts you approve will be visible to that plugin.
        </p>
        <fieldset>
          <legend>Accounts</legend>
          {active.request.accounts.length === 0 ? (
            <p>No connected Provider accounts are available.</p>
          ) : (
            active.request.accounts.map((authorized) => (
              <AccountCheckbox
                key={authorized.account.id}
                authorized={authorized}
                checked={selected.has(authorized.account.id)}
                onChange={toggleAccount}
              />
            ))
          )}
        </fieldset>
        <div className="provider-prompt-actions">
          <button type="button" onClick={() => broker.cancel(active.id)}>
            Cancel
          </button>
          <button
            type="button"
            disabled={selected.size === 0}
            onClick={() => {
              const result = [...selected];
              setSelected(new Set());
              broker.resolve(active.id, result);
            }}
          >
            Approve
          </button>
        </div>
      </div>
    </div>
  );
}

function AccountCheckbox({
  authorized,
  checked,
  onChange
}: {
  authorized: ProviderAuthorizedAccount;
  checked: boolean;
  onChange(accountId: string, checked: boolean): void;
}) {
  const label = authorized.account.displayName ?? authorized.account.username;
  return (
    <label className="provider-account-option">
      <input
        type="checkbox"
        aria-label={label}
        value={authorized.account.id}
        checked={checked}
        onChange={(event) => onChange(authorized.account.id, event.currentTarget.checked)}
      />
      <span>
        {label} · {authorized.instance.displayName}
      </span>
    </label>
  );
}

function focusableElements(root: HTMLElement | null): HTMLElement[] {
  if (root === null) return [];
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])"
    )
  );
}
