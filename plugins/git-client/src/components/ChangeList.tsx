import type { ParsedChangeEntry } from "@git-ramus/contracts";

interface ChangeListProps {
  title: string;
  changes: ParsedChangeEntry[];
  selectedPaths: string[];
  onSelectionChange(paths: string[]): void;
  onViewDiff?(change: ParsedChangeEntry): void;
}

export function ChangeList({
  title,
  changes,
  selectedPaths,
  onSelectionChange,
  onViewDiff
}: ChangeListProps) {
  const visiblePaths = changes.map((change) => change.path);
  const allSelected =
    visiblePaths.length > 0 && visiblePaths.every((path) => selectedPaths.includes(path));

  const toggleAll = () => {
    if (allSelected) {
      onSelectionChange(selectedPaths.filter((path) => !visiblePaths.includes(path)));
      return;
    }
    onSelectionChange(Array.from(new Set([...selectedPaths, ...visiblePaths])));
  };

  const togglePath = (path: string) => {
    onSelectionChange(
      selectedPaths.includes(path)
        ? selectedPaths.filter((selectedPath) => selectedPath !== path)
        : [...selectedPaths, path]
    );
  };

  return (
    <section className="change-group" aria-label={title}>
      <div className="change-group-heading">
        <h3>{title}</h3>
        <label className="select-all">
          <input
            type="checkbox"
            aria-label={`Select all ${title}`}
            checked={allSelected}
            disabled={changes.length === 0}
            onChange={toggleAll}
          />
          Select all
        </label>
      </div>
      {changes.length === 0 ? (
        <p className="muted">No {title.toLowerCase()} changes.</p>
      ) : (
        <ul className="change-list">
          {changes.map((change) => (
            <li key={`${title}:${change.path}`}>
              <label className="change-selection">
                <input
                  type="checkbox"
                  aria-label={`Select ${change.path}`}
                  checked={selectedPaths.includes(change.path)}
                  onChange={() => togglePath(change.path)}
                />
                <span className="change-kind">{change.kind}</span>
                <span className="change-path">{change.path}</span>
              </label>
              {onViewDiff === undefined ? null : (
                <button
                  className="button-link"
                  type="button"
                  aria-label={`View diff for ${change.path}`}
                  onClick={() => onViewDiff(change)}
                >
                  Diff
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
