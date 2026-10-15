// Paths as people read them: the home folder shown as `~`.
import { homeDir } from "@tauri-apps/api/path";

const state = $state<{ home: string | null }>({ home: null });
void homeDir()
  .then((dir) => (state.home = dir.replace(/[\\/]+$/, "")))
  .catch(() => {});

/** `/Users/ana/code/lab1` -> `~/code/lab1`; other paths are returned as they are. */
export function tildify(path: string): string {
  const home = state.home;
  if (!home || !path.startsWith(home)) return path;
  const rest = path.slice(home.length);
  return rest === "" || rest.startsWith("/") || rest.startsWith("\\") ? `~${rest}` : path;
}
