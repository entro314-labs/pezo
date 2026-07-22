import type { QueueTask } from "../types";
import { STAGE_LABELS } from "../types";
import { IconClose } from "./Icons";

interface Props {
  tasks: QueueTask[];
  onCancel: (taskId: number) => void;
  onClear: () => void;
  onClose: () => void;
}

const STATUS_DOT: Record<QueueTask["status"], string> = {
  queued: "bg-edge-strong",
  running: "bg-accent",
  done: "bg-ok",
  failed: "bg-danger",
  cancelled: "bg-t3",
};

function taskDetail(task: QueueTask): string {
  switch (task.status) {
    case "queued":
      return "Queued";
    case "running": {
      const stage = task.stage ? STAGE_LABELS[task.stage] : "Working…";
      return task.progress !== null
        ? `${stage.replace("…", "")} — ${Math.round(task.progress * 100)}%`
        : stage;
    }
    case "done":
      return "Installed";
    case "failed":
      return task.error ?? "Failed";
    case "cancelled":
      return "Cancelled";
  }
}

/** The install queue, anchored above the statusbar like a download manager. */
export function QueuePanel({ tasks, onCancel, onClear, onClose }: Props) {
  const finished = tasks.some((t) => !["queued", "running"].includes(t.status));
  return (
    <div className="fixed right-3 bottom-8 z-40 w-96 overflow-hidden rounded-lg border border-edge-strong bg-surface" style={{ boxShadow: "var(--shadow)" }}>
      <div className="flex h-8 items-center gap-2 border-b border-edge px-3">
        <span className="text-[12px] font-semibold text-t1">Install queue</span>
        <span className="text-[11px] text-t3">
          {tasks.filter((t) => t.status === "queued" || t.status === "running").length} active
        </span>
        <div className="ml-auto flex items-center gap-1">
          {finished && (
            <button onClick={onClear} className="btn-ghost h-6 px-2 text-[11px]">
              Clear finished
            </button>
          )}
          <button onClick={onClose} className="win-btn" aria-label="Close queue">
            <IconClose width={12} height={12} />
          </button>
        </div>
      </div>

      {tasks.length === 0 ? (
        <p className="px-3 py-4 text-center text-[12px] text-t3">
          Nothing queued. Install something from Discoveries.
        </p>
      ) : (
        <ul className="max-h-72 overflow-y-auto">
          {tasks.map((task) => (
            <li
              key={task.id}
              className="flex items-center gap-2.5 border-b border-edge px-3 py-2 last:border-0"
            >
              <span className={`h-2 w-2 shrink-0 rounded-full ${STATUS_DOT[task.status]}`} />
              <div className="min-w-0 flex-1">
                <p className="truncate text-[12px] text-t1">{task.title}</p>
                <p
                  className={`truncate text-[11px] ${
                    task.status === "failed" ? "text-danger select-text" : "text-t3"
                  }`}
                  title={task.error ?? undefined}
                >
                  {taskDetail(task)}
                </p>
                {task.status === "running" && task.progress !== null && (
                  <div className="mt-1 h-1 overflow-hidden rounded-full bg-edge">
                    <div
                      className="h-full rounded-full bg-accent transition-all"
                      style={{ width: `${Math.round(task.progress * 100)}%` }}
                    />
                  </div>
                )}
              </div>
              {task.status === "queued" && (
                <button
                  onClick={() => onCancel(task.id)}
                  className="win-btn shrink-0"
                  title="Remove from queue"
                >
                  <IconClose width={11} height={11} />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
