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
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent text-2xl font-bold text-white">
        ▶
      </div>
      <div>
        <h1 className="text-2xl font-semibold tracking-tight text-t1">Welcome to Pezo</h1>
        <p className="mt-2 max-w-md text-sm leading-relaxed text-t2">
          Pick the folder (ideally a dedicated disk on ext4/btrfs) that will hold your library.
          Pezo creates <code className="text-t1">installers/</code>,{" "}
          <code className="text-t1">library/</code> and <code className="text-t1">prefixes/</code>{" "}
          inside it. Installers already on the disk are found automatically, and more folders can
          be added later in Settings.
        </p>
      </div>
      <button onClick={pick} className="btn-primary px-5 py-2.5">
        Choose library folder
      </button>
    </div>
  );
}
