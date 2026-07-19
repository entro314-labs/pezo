import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  onChosen: (path: string) => void;
}

export function Onboarding({ onChosen }: Props) {
  const pick = async () => {
    const selected = await open({
      directory: true,
      title: "Choose your game library folder (e.g. /mnt/games)",
    });
    if (typeof selected === "string") {
      onChosen(selected);
    }
  };

  return (
    <div className="flex h-full flex-col items-center justify-center gap-6 px-8 text-center">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent-600 text-2xl font-bold text-white">
        ▶
      </div>
      <div>
        <h1 className="text-2xl font-semibold tracking-tight text-white">Welcome to Pezo</h1>
        <p className="mt-2 max-w-md text-sm leading-relaxed text-zinc-400">
          Pick the folder (ideally a dedicated disk) that will hold your library.
          Pezo creates <code className="text-zinc-300">installers/</code>,{" "}
          <code className="text-zinc-300">library/</code> and{" "}
          <code className="text-zinc-300">prefixes/</code> inside it. Drop your GOG
          installers and game folders into <code className="text-zinc-300">installers/</code>,
          then scan.
        </p>
      </div>
      <button
        onClick={pick}
        className="rounded-lg bg-accent-600 px-5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-accent-500"
      >
        Choose library folder
      </button>
    </div>
  );
}
