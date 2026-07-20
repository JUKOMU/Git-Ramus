export interface CloneNavigationEvent {
  readonly id: string;
  readonly route: string;
}

export interface CloneNavigationPublisher {
  publish(route: string): CloneNavigationEvent;
}

export interface CloneNavigationBroker extends CloneNavigationPublisher {
  subscribe(listener: (event: CloneNavigationEvent | null) => void): () => void;
  consume(id: string): void;
  current(): CloneNavigationEvent | null;
  clear(): void;
}

export function createCloneNavigationBroker(): CloneNavigationBroker {
  let current: CloneNavigationEvent | null = null;
  const queued: CloneNavigationEvent[] = [];
  let sequence = 0;
  const listeners = new Set<(event: CloneNavigationEvent | null) => void>();
  const notify = () => {
    for (const listener of [...listeners]) {
      try {
        listener(current);
      } catch {
        listeners.delete(listener);
      }
    }
  };
  return {
    publish(route) {
      validateCloneRoute(route);
      const event = Object.freeze({ id: nextNavigationId(sequence++), route });
      if (current === null) {
        current = event;
        notify();
      } else {
        queued.push(event);
      }
      return event;
    },
    subscribe(listener) {
      listeners.add(listener);
      try {
        listener(current);
      } catch {
        listeners.delete(listener);
      }
      return () => listeners.delete(listener);
    },
    consume(id) {
      if (current?.id !== id) return;
      current = queued.shift() ?? null;
      notify();
    },
    current: () => current,
    clear() {
      if (current === null && queued.length === 0) return;
      current = null;
      queued.length = 0;
      notify();
    }
  };
}

function validateCloneRoute(route: string) {
  if (!/^\/clone\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(route)) {
    throw new Error("Clone navigation route is invalid");
  }
}

function nextNavigationId(sequence: number): string {
  const randomUuid = globalThis.crypto?.randomUUID;
  return typeof randomUuid === "function"
    ? randomUuid.call(globalThis.crypto)
    : `clone-navigation-${sequence}`;
}

export const cloneNavigationBroker = createCloneNavigationBroker();
