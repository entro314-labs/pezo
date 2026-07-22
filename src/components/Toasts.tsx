export type ToastVariant = "info" | "success" | "error";

export interface Toast {
  id: number;
  message: string;
  variant: ToastVariant;
}

const VARIANT_STYLES: Record<ToastVariant, string> = {
  info: "border-edge bg-surface text-t1",
  success: "border-ok/30 bg-surface text-t1",
  error: "border-danger/40 bg-surface text-t1",
};

const VARIANT_DOT: Record<ToastVariant, string> = {
  info: "bg-accent",
  success: "bg-ok",
  error: "bg-danger",
};

export function Toasts({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;
  return (
    <div className="fixed bottom-10 left-1/2 z-50 flex w-[26rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`flex items-start gap-3 rounded-xl border px-4 py-3 text-sm shadow-lg ${VARIANT_STYLES[toast.variant]}`}
          style={{ boxShadow: "var(--shadow)" }}
        >
          <span
            className={`mt-1.5 h-2 w-2 shrink-0 rounded-full ${VARIANT_DOT[toast.variant]}`}
          />
          <p className="min-w-0 flex-1 leading-relaxed break-words select-text">
            {toast.message}
          </p>
          <button
            onClick={() => onDismiss(toast.id)}
            className="shrink-0 text-t3 hover:text-t1"
            aria-label="Dismiss"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
