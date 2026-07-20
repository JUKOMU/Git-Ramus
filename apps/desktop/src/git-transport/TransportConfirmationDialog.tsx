import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type { ActiveGitTransportPrompt, GitTransportPromptBroker } from "./promptBroker";

interface TransportConfirmationDialogProps {
  broker: GitTransportPromptBroker;
}

export function TransportConfirmationDialog({ broker }: TransportConfirmationDialogProps) {
  const [active, setActive] = useState<ActiveGitTransportPrompt | null>(null);
  const activeRef = useRef<ActiveGitTransportPrompt | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const unsubscribe = broker.subscribe((next) => {
      activeRef.current = next;
      setActive(next);
    });
    return () => {
      const current = activeRef.current;
      activeRef.current = null;
      unsubscribe();
      if (current !== null) broker.unavailable(current.id);
    };
  }, [broker]);

  useEffect(() => {
    if (active === null) {
      previousFocusRef.current?.focus();
      previousFocusRef.current = null;
      return;
    }
    if (previousFocusRef.current === null) {
      previousFocusRef.current =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    dialogRef.current?.querySelector<HTMLElement>("button")?.focus();
  }, [active]);

  if (active === null) return null;
  const copy = promptCopy(active);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      broker.cancel(active.id);
      return;
    }
    if (event.key !== "Tab") return;
    const controls = focusableElements(dialogRef.current);
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  return (
    <div className="provider-prompt-overlay transport-prompt-overlay" role="presentation">
      <div
        ref={dialogRef}
        className="provider-prompt-dialog transport-prompt-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="transport-prompt-title"
        aria-describedby="transport-prompt-guidance"
        data-testid="transport-confirmation-dialog"
        onKeyDown={onKeyDown}
      >
        <h2 id="transport-prompt-title">{copy.title}</h2>
        <p id="transport-prompt-guidance">{copy.guidance}</p>
        <p className="provider-prompt-context transport-prompt-caller">
          Requested by: <strong>{active.request.pluginId}</strong>
        </p>
        <p className="provider-prompt-context transport-prompt-resource">
          Target: <strong>{active.request.resourceLabel}</strong>
        </p>
        <div className="provider-prompt-actions transport-prompt-actions">
          <button type="button" onClick={() => broker.cancel(active.id)}>
            Cancel
          </button>
          <button type="button" onClick={() => broker.resolve(active.id, true)}>
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}

function promptCopy(active: ActiveGitTransportPrompt): { title: string; guidance: string } {
  switch (active.request.kind) {
    case "network":
      return {
        title: "Confirm Git network operation",
        guidance: `Git-Ramus will run an interactive Git ${active.request.operation} operation. System credential or SSH Agent prompts may appear.`
      };
    case "sourceTrust":
      return {
        title: "Trust Git clone source",
        guidance:
          "Cloning downloads repository-controlled content. Initial checkout runs with hooks and external attribute drivers disabled."
      };
    case "replaceConfig":
      return {
        title: "Replace repository Git configuration",
        guidance:
          "The selected transport profile will replace conflicting repository transport settings after a protected snapshot is created."
      };
  }
}

function focusableElements(root: HTMLElement | null): HTMLElement[] {
  if (root === null) return [];
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])"
    )
  );
}
