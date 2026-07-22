import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { AppConfig, FsInfo, ToolStatus } from "../types";

interface Props {
  config: AppConfig;
  tools: ToolStatus[];
  fsInfo: FsInfo | null;
  onLibraryChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}

export function SetupView({ config, tools, fsInfo, onLibraryChanged, onError, onNotice }: Props) {
  const [installable, setInstallable] = useState<Set<string>>(new Set());
  const [installingTool, setInstallingTool] = useState<string | null>(null);

  useEffect(() => {
    api
      .installableTools()
      .then((names) => setInstallable(new Set(names)))
      .catch(() => setInstallable(new Set()));
  }, []);

  const installTool = async (name: string) => {
    setInstallingTool(name);
    try {
      await api.installTool(name);
      onNotice(`Installed ${name}`);
      onLibraryChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setInstallingTool(null);
    }
  };
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
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <div className="toolbar">
        <h2 className="text-sm font-semibold text-t1">Settings</h2>
      </div>

      <div className="min-h-0 max-w-3xl flex-1 overflow-y-auto p-5">
        <section className="card mb-6 p-5">
          <h3 className="text-sm font-semibold text-t1">Library</h3>
          <div className="mt-3 flex items-center justify-between gap-4">
            <code className="truncate text-sm text-t2 select-text">{config.library_root}</code>
            <button onClick={changeLibrary} className="btn-secondary shrink-0 py-1.5 text-xs">
              Change
            </button>
          </div>
          {fsInfo?.problematic && (
            <div className="mt-3 rounded-lg border border-danger/30 bg-danger-soft px-3 py-2.5 text-xs leading-relaxed text-danger">
              This library is on a <strong>{fsInfo.fstype}</strong> filesystem. Proton and Wine
              misbehave here — prefixes need symlinks and unix permissions that {fsInfo.fstype}{" "}
              can't provide. Move the library to a Linux filesystem (ext4/btrfs) and add this
              disk as a scan folder instead.
            </div>
          )}
          <p className="mt-3 text-xs leading-relaxed text-t3">
            Use ext4/btrfs for the library disk. The database lives on the disk itself, so the
            whole library survives reinstalls and machine moves.
          </p>
          <label className="mt-4 flex items-start gap-2 text-sm text-t1">
            <input
              type="checkbox"
              checked={config.adopt_portables}
              className="mt-0.5"
              onChange={async (e) => {
                try {
                  await api.setAdoptPortables(e.target.checked);
                  onLibraryChanged();
                } catch (err) {
                  onError(String(err));
                }
              }}
            />
            <span>
              Copy portable games into the library on install
              <span className="block text-xs text-t3">
                Keeps them playable when the source disk is unplugged. Off = run them in place.
              </span>
            </span>
          </label>
        </section>

        <section className="card mb-6 p-5">
          <div className="flex items-center justify-between gap-4">
            <h3 className="text-sm font-semibold text-t1">Scan folders</h3>
            <button onClick={addScanDir} className="btn-secondary shrink-0 py-1.5 text-xs">
              Add folder
            </button>
          </div>
          <p className="mt-1 text-xs text-t3">
            Extra folders searched for installers — other disks, download folders. Nothing is
            installed to or written in them.
          </p>
          {config.scan_dirs.length > 0 && (
            <ul className="mt-4 flex flex-col gap-2">
              {config.scan_dirs.map((dir) => (
                <li key={dir} className="flex items-center justify-between gap-4">
                  <code className="truncate text-sm text-t2 select-text">{dir}</code>
                  <button
                    onClick={() => removeScanDir(dir)}
                    className="btn-ghost shrink-0 px-2 py-1 text-xs hover:text-danger"
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="card p-5">
          <h3 className="text-sm font-semibold text-t1">External tools</h3>
          <p className="mt-1 text-xs text-t3">
            Pezo orchestrates proven tools instead of reimplementing them.
          </p>
          <ul className="mt-4 flex flex-col gap-3">
            {tools.map((tool) => (
              <li key={tool.name} className="flex items-start gap-3">
                <span
                  className={`mt-0.5 h-2.5 w-2.5 shrink-0 rounded-full ${
                    tool.found ? "bg-ok" : tool.required ? "bg-danger" : "bg-edge-strong"
                  }`}
                />
                <div className="min-w-0 flex-1">
                  <p className="text-sm text-t1">
                    {tool.name}
                    {!tool.found && tool.required && (
                      <span className="ml-2 text-xs text-danger">required</span>
                    )}
                    {!tool.found && !tool.required && (
                      <span className="ml-2 text-xs text-t3">optional</span>
                    )}
                  </p>
                  <p className="text-xs text-t2">{tool.purpose}</p>
                  <p className="mt-0.5 truncate text-xs text-t3 select-text">
                    {tool.found && tool.path ? tool.path : tool.hint}
                  </p>
                </div>
                {!tool.found && installable.has(tool.name) && (
                  <button
                    onClick={() => installTool(tool.name)}
                    disabled={installingTool !== null}
                    className="btn-secondary h-6 shrink-0 px-2.5 text-[12px]"
                    title="Installs through your package manager — the system will ask for your password"
                  >
                    {installingTool === tool.name ? "Installing…" : "Install"}
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}
