import type { Job } from "@git-ramus/contracts";
import type { HostApi } from "../lib/hostApi";

interface TaskCenterProps {
  jobs: Job[];
  hostApi: HostApi;
}

export function TaskCenter({ jobs, hostApi }: TaskCenterProps) {
  return (
    <aside className="task-rail" aria-label="Task center">
      <h2>Tasks</h2>
      {jobs.length === 0 ? <p>No active tasks</p> : null}
      {jobs.map((job) => (
        <article key={job.id}>
          <strong>{job.title}</strong>
          <span>{Math.round(job.progress * 100)}%</span>
          {job.status === "queued" || job.status === "running" ? (
            <button type="button" onClick={() => void hostApi.cancelJob(job.id)}>
              Cancel
            </button>
          ) : null}
        </article>
      ))}
    </aside>
  );
}
