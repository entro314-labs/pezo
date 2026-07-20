import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { AppConfig, ToolStatus } from "../types";

interface Props {
  config: AppConfig;
  tools: ToolStatus[];
  onLibraryChanged: () => void;
  onError: (message: string) => void;
}

export function SetupView({ config, tools, onLibraryChanged, onError }: Props) {
  const changeLibrary = async () => {
    try {
      const selected = await open({ directory: true, title: "Choose library folder" });
      if (typeof selected === "string") {
        await api.setLibraryRoot(selected);
        onLibraryChanged();
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const addScanDir = async () => {
    try {
      const selected = await open({ directory: true, title: "Add scan folder" });
      if (typeof selected === "string") {
        await api.addScanDir(selected);
        onLibraryChanged();
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const removeScanDir = async (path: string) => {
    try {
      await api.removeScanDir(path);
      onLibraryChanged();
    } catch (e) {
      onError(String(e));
    }
  };

  return (
    <div className="max-w-3xl p-8">
      <h2 className="mb-6 text-xl font-semibold tracking-tight text-white">Setup</h2>

      <section className="mb-8 rounded-xl border border-ink-700 bg-ink-900 p-5">
        <h3 className="text-sm font-medium text-zinc-300">Library</h3>
        <div className="mt-3 flex items-center justify-between gap-4">
          <code className="truncate text-sm text-zinc-400">{config.library_root}</code>
          <button
            onClick={changeLibrary}
            className="shrink-0 rounded-md bg-ink-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-ink-800"
          >
            Change
          </button>
        </div>
        <p className="mt-3 text-xs leading-relaxed text-zinc-600">
          Use ext4 for the library disk — Proton and Wine misbehave on NTFS. The database
          lives on the disk itself, so the whole library survives reinstalls and machine moves.
        </p>
      </section>

      <section className="mb-8 rounded-xl border border-ink-700 bg-ink-900 p-5">
        <div className="flex items-center justify-between gap-4">
          <h3 className="text-sm font-medium text-zinc-300">Scan folders</h3>
          <button
            onClick={addScanDir}
            className="shrink-0 rounded-md bg-ink-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-ink-800"
          >
            Add folder
          </button>
        </div>
        <p className="mt-1 text-xs text-zinc-600">
          Extra folders searched for installers — other disks, download folders. Nothing is
          installed to or written in them.
        </p>
        {config.scan_dirs.length > 0 && (
          <ul className="mt-4 flex flex-col gap-2">
            {config.scan_dirs.map((dir) => (
              <li key={dir} className="flex items-center justify-between gap-4">
                <code className="truncate text-sm text-zinc-400">{dir}</code>
                <button
                  onClick={() => removeScanDir(dir)}
                  className="shrink-0 rounded-md px-2 py-1 text-xs font-medium text-zinc-500 hover:bg-ink-800 hover:text-red-400"
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="rounded-xl border border-ink-700 bg-ink-900 p-5">
        <h3 className="text-sm font-medium text-zinc-300">External tools</h3>
        <p className="mt-1 text-xs text-zinc-600">
          Pezo orchestrates proven tools instead of reimplementing them.
        </p>
        <ul className="mt-4 flex flex-col gap-3">
          {tools.map((tool) => (
            <li key={tool.name} className="flex items-start gap-3">
              <span
                className={`mt-0.5 h-2.5 w-2.5 shrink-0 rounded-full ${
                  tool.found ? "bg-emerald-400" : tool.required ? "bg-red-400" : "bg-zinc-600"
                }`}
              />
              <div className="min-w-0">
                <p className="text-sm text-zinc-200">
                  {tool.name}
                  {!tool.found && tool.required && (
                    <span className="ml-2 text-xs text-red-400">required</span>
                  )}
                  {!tool.found && !tool.required && (
                    <span className="ml-2 text-xs text-zinc-500">optional</span>
                  )}
                </p>
                <p className="text-xs text-zinc-500">{tool.purpose}</p>
                <p className="mt-0.5 truncate text-xs text-zinc-600">
                  {tool.found && tool.path ? tool.path : tool.hint}
                </p>
              </div>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
