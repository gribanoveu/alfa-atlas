import { useCallback, useEffect, useRef, useState } from "react";
import {
  acceptRootSuggestion,
  addExtraRoot,
  getExtraRoots,
  removeExtraRoot,
  suggestExtraRoots,
  type ExtraRoot,
  type RootSuggestion,
} from "../lib/aiTools";
import { toMessage } from "../lib/errors";

/** "No project is open" comes back as an ordinary command error string —
 * the one failure this list treats as a state rather than a fault, same as
 * `useToolPermissions`. */
function isNoProject(message: string): boolean {
  return message.includes("no project is open");
}

/** Turns a picked folder into the `@deps` segment that will address it:
 * the folder's own name, with anything that cannot be one path segment
 * replaced, and a numeric suffix when that name is already taken.
 *
 * Derived here rather than asked for, because the name is plumbing — what
 * the user chose is the folder. It stays visible and removable in the list,
 * so a name that reads badly is one delete away from being re-added under a
 * renamed folder. `add_extra_root` validates it again on the Rust side; this
 * only keeps the common case from bouncing off that validation. */
export function deriveRootName(path: string, taken: string[]): string {
  const base = path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() ?? "";
  const cleaned = base.replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^[.-]+/, "");
  const stem = cleaned.length > 0 ? cleaned : "source";
  if (!taken.includes(stem)) return stem;
  for (let n = 2; ; n += 1) {
    const candidate = `${stem}-${n}`;
    if (!taken.includes(candidate)) return candidate;
  }
}

/** The open project's external read-only source roots, plus the add/remove
 * writes behind the Settings list. Same shape as `useToolPermissions`:
 * load, degrade on "no project", write one row at a time. */
export function useExtraRoots() {
  const [roots, setRoots] = useState<ExtraRoot[]>([]);
  const [suggestions, setSuggestions] = useState<RootSuggestion[]>([]);
  /** What the last accepted suggestion did, when it is worth reporting. */
  const [note, setNote] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [noProject, setNoProject] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The root name whose write is in flight, or `"+"` while adding — lets
   * the caller disable just that row instead of the whole card. */
  const [pending, setPending] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  /** Suggestions depend on what is already configured, so they are re-read
   * after every write rather than adjusted in place here — one cheap call
   * that cannot drift from the rule the Rust side actually applies. */
  const refreshSuggestions = useCallback(() => {
    void suggestExtraRoots()
      .then((next) => {
        if (mounted.current) setSuggestions(next);
      })
      .catch(() => {
        // A project that cannot be probed simply offers nothing; the roots
        // list above already reports anything worth reporting.
        if (mounted.current) setSuggestions([]);
      });
  }, []);

  useEffect(() => {
    refreshSuggestions();
  }, [refreshSuggestions]);

  useEffect(() => {
    void getExtraRoots()
      .then((next) => {
        if (!mounted.current) return;
        setRoots(next);
        setNoProject(false);
        setError(null);
      })
      .catch((e) => {
        if (!mounted.current) return;
        const message = toMessage(e);
        if (isNoProject(message)) setNoProject(true);
        else setError(message);
      })
      .finally(() => {
        if (mounted.current) setLoading(false);
      });
  }, []);

  /** `add` needs the current names to pick a free one, but must not be
   * re-created on every list change — a changing callback identity would
   * re-render every consumer for nothing. */
  const rootsRef = useRef<ExtraRoot[]>([]);
  rootsRef.current = roots;

  const addRoot = useCallback(
    async (name: string, path: string) => {
      setPending("+");
      setError(null);
      try {
        await addExtraRoot(name, path);
        if (!mounted.current) return;
        setRoots((prev) => [...prev, { name, path }]);
        refreshSuggestions();
      } catch (e) {
        if (mounted.current) setError(toMessage(e));
      } finally {
        if (mounted.current) setPending(null);
      }
    },
    [refreshSuggestions],
  );

  /** A folder the user picked: the name is derived from it. */
  const add = useCallback(
    (path: string) =>
      addRoot(
        deriveRootName(
          path,
          rootsRef.current.map((r) => r.name),
        ),
        path,
      ),
    [addRoot],
  );

  /** A detected root. The backend owns what accepting means — for Java it
   * unpacks the sources first — so the resulting list is re-read rather than
   * guessed at here. */
  const addSuggested = useCallback(async (suggestion: RootSuggestion) => {
    setPending("+");
    setError(null);
    setNote(null);
    try {
      const what = await acceptRootSuggestion(suggestion.kind);
      if (!mounted.current) return;
      setNote(what.length > 0 ? what : null);
      setRoots(await getExtraRoots());
      refreshSuggestions();
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    } finally {
      if (mounted.current) setPending(null);
    }
  }, [refreshSuggestions]);

  const remove = useCallback(async (name: string) => {
    setPending(name);
    setError(null);
    try {
      await removeExtraRoot(name);
      if (!mounted.current) return;
      setRoots((prev) => prev.filter((r) => r.name !== name));
      refreshSuggestions();
    } catch (e) {
      if (mounted.current) setError(toMessage(e));
    } finally {
      if (mounted.current) setPending(null);
    }
  }, [refreshSuggestions]);

  return {
    roots,
    suggestions,
    loading,
    noProject,
    error,
    note,
    pending,
    add,
    addSuggested,
    remove,
  };
}
