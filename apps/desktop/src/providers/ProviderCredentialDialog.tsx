import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type { ActivePrompt, PromptBroker } from "./promptBroker";
import type { ProviderCredentialPromptRequest } from "./promptPorts";

interface ProviderCredentialDialogProps {
  broker: PromptBroker<ProviderCredentialPromptRequest, string>;
}

export function ProviderCredentialDialog({ broker }: ProviderCredentialDialogProps) {
  const [active, setActive] = useState<ActivePrompt<ProviderCredentialPromptRequest> | null>(null);
  const [value, setValue] = useState("");
  const activeRef = useRef<ActivePrompt<ProviderCredentialPromptRequest> | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const unsubscribe = broker.subscribe((next) => {
      activeRef.current = next;
      setActive(next);
      if (next === null) setValue("");
    });
    return () => {
      const current = activeRef.current;
      activeRef.current = null;
      unsubscribe();
      setValue("");
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
    const input = dialogRef.current?.querySelector<HTMLElement>("input");
    input?.focus();
    return () => {
      if (activeRef.current === null) previousFocusRef.current?.focus();
    };
  }, [active]);

  if (active === null) return null;
  const title =
    active.request.purpose === "rotate" ? "Rotate Provider account" : "Connect Provider account";
  const action = active.request.purpose === "rotate" ? "Rotate" : "Connect";

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

  return (
    <div className="provider-prompt-overlay" role="presentation">
      <div
        ref={dialogRef}
        className="provider-prompt-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="provider-credential-title"
        aria-describedby="provider-credential-guidance"
        data-testid="provider-credential-dialog"
        onKeyDown={onKeyDown}
      >
        <h2 id="provider-credential-title">{title}</h2>
        <p id="provider-credential-guidance">
          Use a read-only personal access token with the smallest Provider scope needed for this
          task. The token stays in the trusted host and is never sent to a plugin.
        </p>
        <p className="provider-prompt-context">
          Provider: <strong>{active.request.providerLabel}</strong>
          {active.request.accountLabel === null ? null : (
            <>
              {" "}
              Account: <strong>{active.request.accountLabel}</strong>
            </>
          )}
        </p>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (value.length === 0) return;
            const result = value;
            setValue("");
            broker.resolve(active.id, result);
          }}
        >
          <label htmlFor="provider-pat-input">Personal access token</label>
          <input
            id="provider-pat-input"
            type="password"
            value={value}
            onChange={(event) => setValue(event.currentTarget.value)}
            autoComplete="off"
            spellCheck={false}
            required
          />
          <div className="provider-prompt-actions">
            <button type="button" onClick={() => broker.cancel(active.id)}>
              Cancel
            </button>
            <button type="submit" disabled={value.length === 0}>
              {action}
            </button>
          </div>
        </form>
      </div>
    </div>
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
