// Dev-only mock backend: `pnpm dev` + open http://localhost:1420/?mock
// Lets the UI render in a plain browser (no Tauri) for design work.
import type { AppConfig, Candidate, Game, QueueTask, WineBuild, WineRelease } from "./types";

/** Tiny event bus standing in for Tauri's event system. */
const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();

export function mockListen(
  event: string,
  handler: (event: { payload: unknown }) => void,
): () => void {
  const set = listeners.get(event) ?? new Set();
  set.add(handler);
  listeners.set(event, set);
  return () => set.delete(handler);
}

function emitMock(event: string, payload: unknown) {
  for (const handler of listeners.get(event) ?? []) {
    handler({ payload });
  }
}

/** Simulated install queue: tasks progress through stages on timers. */
const mockQueue: QueueTask[] = [];
let nextTaskId = 1;
let queueBusy = false;

function pumpQueue() {
  if (queueBusy) return;
  const task = mockQueue.find((t) => t.status === "queued");
  if (!task) return;
  queueBusy = true;
  task.status = "running";
  task.stage = "extracting";
  emitMock("queue-changed", [...mockQueue]);
  let ticks = 0;
  const timer = setInterval(() => {
    ticks += 1;
    task.progress = Math.min(ticks / 8, 0.99);
    if (ticks === 6) task.stage = "registering";
    if (ticks >= 8) {
      clearInterval(timer);
      task.status = task.title.includes("M&M") ? "failed" : "done";
      task.error = task.status === "failed" ? "no launchable game found inside" : null;
      task.stage = null;
      task.progress = null;
      queueBusy = false;
      emitMock("queue-changed", [...mockQueue]);
      pumpQueue();
      return;
    }
    emitMock("queue-changed", [...mockQueue]);
  }, 700);
}

function svgCover(title: string, hue: number): string {
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='600' height='900'>
    <defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'>
      <stop offset='0' stop-color='hsl(${hue} 55% 32%)'/>
      <stop offset='1' stop-color='hsl(${(hue + 50) % 360} 60% 14%)'/>
    </linearGradient></defs>
    <rect width='600' height='900' fill='url(#g)'/>
    <text x='50%' y='52%' text-anchor='middle' font-family='sans-serif' font-size='52'
      font-weight='700' fill='rgba(255,255,255,0.85)'>${title}</text>
  </svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

const games: Game[] = [
  ["Alpha Protocol", 71214, "gog", 12600, "2026-07-12 20:11:00"],
  ["Project Wingman", 895870, null, 44280, "2026-07-18 22:40:00"],
  ["DREDGE", 1562430, "gog", 7300, null],
  ["Katana ZERO", 460950, "gog", 0, null],
  ["Tiny Glade", 2198150, null, 1900, "2026-06-30 18:03:00"],
  ["Sailwind", 1764530, null, 0, null],
  ["The Chronicles of Riddick", 0, "gog", 31000, "2026-05-02 23:59:00"],
  ["Tyrian 2000", 0, null, 800, null],
].map(([title, id, store, secs, played], i) => ({
  id: i + 1,
  slug: String(title).toLowerCase().replace(/[^a-z0-9]+/g, "-"),
  title: String(title),
  runner: i === 7 ? "dosbox" : ("proton" as const),
  install_path: `/mnt/games/library/${String(title).toLowerCase()}`,
  launch_path: `/mnt/games/library/${String(title).toLowerCase()}/game.exe`,
  prefix_path: i === 7 ? null : `/mnt/games/prefixes/${String(title).toLowerCase()}`,
  source_path: `/run/media/user/WD 6TB/GOG/${String(title)}`,
  added_at: "2026-07-01 10:00:00",
  last_played_at: played as string | null,
  play_seconds: secs as number,
  store_id: id ? String(id) : null,
  store: store as string | null,
  launch_args: "",
  env_vars: "",
  wrappers: i === 1 ? "gamemode,mangohud" : "",
  wine_build: i === 1 ? "GE-Proton11-7" : null,
})) as Game[];

const kinds = [
  "gog-installer",
  "gog-installer",
  "archive",
  "archive",
  "portable-windows",
  "windows-installer",
  "unknown",
  "dos-game",
  "gog-installer",
  "archive",
] as const;

const candidates: Candidate[] = Array.from({ length: 10 }, (_, i) => ({
  id: i + 1,
  path:
    i % 3 === 0
      ? `/run/media/user/WD 6TB/GOG/setup_sample_game_${i}.exe`
      : `/run/media/user/Steam and Gaming/Sample.Game.${i}.rar`,
  kind: kinds[i],
  display_name: [
    "Shadows of Doubt",
    "Roboquest",
    "Echo Point Nova",
    "Call of Duty Black Ops II",
    "Hannah",
    "BGamer",
    "M&M(L)",
    "Tyrian 2000",
    "Ys X Nordics",
    "Ghost Exile",
  ][i],
  size_bytes: [4.2e9, 6.1e9, 3.0e9, 11.2e9, 8.4e9, 1.1e8, 5.6e9, 4.1e7, 9.9e9, 6.4e9][i],
  discovered_at: "2026-07-19 12:00:00",
  installed_game_id: i === 1 ? 3 : null,
}));

const config: AppConfig = {
  library_root: "/mnt/games",
  scan_dirs: ["/run/media/user/WD 6TB/GOG", "/run/media/user/Steam and Gaming"],
  adopt_portables: true,
  default_wine: null,
  interactive_installers: false,
};

const wineBuilds: WineBuild[] = [
  { name: "GE-Proton11-7", kind: "proton", path: "/home/user/.local/share/Steam/compatibilitytools.d/GE-Proton11-7", source: "compat-tools", removable: true },
  { name: "proton-cachyos-slr", kind: "proton", path: "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr", source: "compat-tools", removable: false },
  { name: "wine-11.19-amd64-wow64", kind: "wine", path: "/home/user/.local/share/pezo/wine/wine-11.19-amd64-wow64", source: "pezo", removable: true },
  { name: "system", kind: "wine", path: "/usr", source: "system", removable: false },
];

const covers: Record<number, string> = Object.fromEntries(
  games.map((g, i) => [g.id, svgCover(g.title.split(" ")[0], (i * 67) % 360)]),
);

export const mockEnabled =
  typeof window !== "undefined" && new URLSearchParams(window.location.search).has("mock");

export async function mockInvoke<T>(cmd: string, args?: unknown): Promise<T> {
  await new Promise((r) => setTimeout(r, 60));
  switch (cmd) {
    case "get_config":
      return config as T;
    case "doctor":
      return [
        { name: "umu-launcher", found: true, path: "/usr/bin/umu-run", purpose: "Runs Windows games with Proton outside Steam", hint: "", required: true },
        { name: "innoextract", found: false, path: null, purpose: "Unpacks GOG offline installers without running them", hint: "Arch: pacman -S innoextract", required: true },
        { name: "7-Zip", found: true, path: "/usr/bin/7z", purpose: "Extracts split archives and stubborn RARs", hint: "", required: false },
        { name: "GameMode", found: true, path: "/usr/bin/gamemoderun", purpose: "CPU/GPU performance governor while a game runs", hint: "", required: false },
      ] as T;
    case "library_fs_info":
      return { fstype: "btrfs", problematic: false } as T;
    case "desktop_layout":
      return { left: [], right: ["minimize", "maximize", "close"] } as T;
    case "installable_tools":
      return ["innoextract", "unrar"] as T;
    case "install_tool":
      await new Promise((r) => setTimeout(r, 800));
      return undefined as T;
    case "queue_install": {
      const { candidateId, archive } = args as { candidateId: number; archive?: string | null };
      const existing = mockQueue.find(
        (t) => t.candidate_id === candidateId && (t.status === "queued" || t.status === "running"),
      );
      if (existing) return existing.id as T;
      const candidate = candidates.find((c) => c.id === candidateId);
      const task: QueueTask = {
        id: nextTaskId++,
        candidate_id: candidateId,
        title: candidate?.display_name ?? `Candidate ${candidateId}`,
        archive: archive ?? null,
        status: "queued",
        stage: null,
        progress: null,
        error: null,
        game_id: null,
      };
      mockQueue.push(task);
      emitMock("queue-changed", [...mockQueue]);
      setTimeout(pumpQueue, 300);
      return task.id as T;
    }
    case "queue_state":
      return [...mockQueue] as T;
    case "queue_cancel": {
      const { taskId } = args as { taskId: number };
      const task = mockQueue.find((t) => t.id === taskId);
      if (task && task.status === "queued") {
        task.status = "cancelled";
        emitMock("queue-changed", [...mockQueue]);
        return true as T;
      }
      return false as T;
    }
    case "queue_clear_finished": {
      for (let i = mockQueue.length - 1; i >= 0; i--) {
        if (mockQueue[i].status !== "queued" && mockQueue[i].status !== "running") {
          mockQueue.splice(i, 1);
        }
      }
      emitMock("queue-changed", [...mockQueue]);
      return undefined as T;
    }
    case "list_games":
      return games as T;
    case "list_candidates":
      return candidates as T;
    case "get_cover":
      return (covers[(args as { gameId: number }).gameId] ?? null) as T;
    case "scan_installers":
      return { discovered: candidates, ignored: 4, pruned: 0, offline: [] } as T;
    case "list_wine_builds":
      return wineBuilds as T;
    case "wine_sources":
      return [
        { id: "ge-proton", name: "GE-Proton", kind: "proton", description: "Proton with extra media codecs and game fixes — the usual first pick" },
        { id: "wine", name: "Wine", kind: "wine", description: "Upstream Wine (Kron4ek build, WoW64 — no 32-bit system libraries needed)" },
      ] as T;
    case "list_wine_releases": {
      const { source } = args as { source: string };
      return [7, 6, 5].map((n) => ({
        source,
        tag: source === "wine" ? `11.1${n}` : `GE-Proton11-${n}`,
        asset: source === "wine" ? `wine-11.1${n}-amd64-wow64.tar.xz` : `GE-Proton11-${n}-x86_64.tar.gz`,
        size: 5.4e8,
        published_at: `2026-10-0${n}T12:00:00Z`,
        prerelease: false,
        installed: source !== "wine" && n === 7,
      })) as WineRelease[] as T;
    }
    case "install_wine_build": {
      const { tag } = args as { source: string; tag: string };
      const total = 5.4e8;
      for (let i = 1; i <= 5; i++) {
        await new Promise((r) => setTimeout(r, 300));
        emitMock("wine-install-progress", { source: "", tag, progress: { stage: "downloading", done: (total * i) / 5, total } });
      }
      emitMock("wine-install-progress", { source: "", tag, progress: { stage: "extracting" } });
      await new Promise((r) => setTimeout(r, 400));
      const build: WineBuild = { name: tag, kind: "proton", path: `/home/user/.local/share/Steam/compatibilitytools.d/${tag}`, source: "compat-tools", removable: true };
      wineBuilds.unshift(build);
      return build as T;
    }
    case "remove_wine_build": {
      const { name } = args as { name: string };
      const index = wineBuilds.findIndex((b) => b.name === name);
      if (index >= 0) wineBuilds.splice(index, 1);
      if (config.default_wine === name) config.default_wine = null;
      return config as T;
    }
    case "set_default_wine":
      config.default_wine = (args as { name: string | null }).name;
      return config as T;
    case "list_candidate_archives":
      return [
        "/run/media/user/WD 6TB/Games/M&M(L)/mm6.iso",
        "/run/media/user/WD 6TB/Games/M&M(L)/mm7.iso",
      ] as T;
    default:
      return undefined as T;
  }
}
