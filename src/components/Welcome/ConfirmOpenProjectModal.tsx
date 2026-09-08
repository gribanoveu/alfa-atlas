import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import { toMessage } from "../../lib/errors";
import { ensureAtlasGitignore, probeOpenPath } from "../../lib/project";
import type { DocsCandidate, ProbeResult } from "../../lib/project";
import {
  gitCheckoutBranch,
  gitCheckoutRemoteBranch,
  gitListBranches,
  localBranchName,
  openableBranches,
  type GitBranchInfo,
} from "../../lib/git";
import "./CloneRepoModal.css";

type ConfirmOpenProjectModalProps = {
  probe: ProbeResult;
  onCancel: () => void;
  onConfirm: (docsRoot: string) => Promise<void>;
};

function folderName(path: string): string {
  return path.split(/[/\\]/).filter(Boolean).pop() ?? path;
}

export function ConfirmOpenProjectModal({
  probe,
  onCancel,
  onConfirm,
}: ConfirmOpenProjectModalProps) {
  const [docsRoot, setDocsRoot] = useState(
    probe.suggestedDocsRoot ?? probe.candidates[0]?.path ?? "",
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [candidatesOpen, setCandidatesOpen] = useState(false);
  const [addToGitignore, setAddToGitignore] = useState(true);
  const candidatesRef = useRef<HTMLDivElement>(null);
  /** Candidates re-scanned after a branch switch; the probe's own list until then. */
  const [candidates, setCandidates] = useState<DocsCandidate[]>(probe.candidates);
  const [branches, setBranches] = useState<GitBranchInfo[]>([]);
  const [branchesOpen, setBranchesOpen] = useState(false);
  const [switching, setSwitching] = useState(false);
  const branchesRef = useRef<HTMLDivElement>(null);

  const selectedCandidate = candidates.find((c) => c.path === docsRoot) ?? null;

  /** A plain folder is not a repository — no branches, no picker, no error. */
  useEffect(() => {
    let cancelled = false;
    gitListBranches(probe.root)
      .then((list) => {
        if (!cancelled) setBranches(list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [probe.root]);

  const offeredBranches = useMemo(() => openableBranches(branches), [branches]);
  const currentBranch = branches.find((b) => b.isCurrent)?.name ?? "";

  /** Checking out rewrites the working tree, so the docs scan has to run
   * again — a template branch is precisely the case where the previous
   * branch had nothing to find. */
  const switchBranch = async (name: string) => {
    const target = offeredBranches.find((b) => b.name === name);
    if (!target || target.isCurrent) return;
    setSwitching(true);
    setError(null);
    try {
      if (target.isRemote) {
        await gitCheckoutRemoteBranch(probe.root, target.name);
      } else {
        await gitCheckoutBranch(probe.root, target.name);
      }
      const rescanned = await probeOpenPath(probe.root);
      setCandidates(rescanned.candidates);
      setDocsRoot(rescanned.suggestedDocsRoot ?? rescanned.docsRoot ?? "");
      setBranches(await gitListBranches(probe.root));
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setSwitching(false);
    }
  };

  useEffect(() => {
    if (!candidatesOpen && !branchesOpen) return;

    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!candidatesRef.current?.contains(target)) setCandidatesOpen(false);
      if (!branchesRef.current?.contains(target)) setBranchesOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setCandidatesOpen(false);
      setBranchesOpen(false);
    };

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [branchesOpen, candidatesOpen]);

  const docsUnderRepo = useMemo(() => {
    if (!docsRoot) return false;
    const root = probe.root.replace(/[/\\]+$/, "");
    const docs = docsRoot.replace(/[/\\]+$/, "");
    return (
      docs === root ||
      docs.startsWith(`${root}/`) ||
      docs.startsWith(`${root}\\`)
    );
  }, [docsRoot, probe.root]);

  const pickDocsFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Корень документации",
      defaultPath: probe.root,
    });
    if (selected === null || Array.isArray(selected)) return;
    setDocsRoot(selected);
    setError(null);
  };

  const submit = async () => {
    if (!docsRoot.trim()) {
      setError("Укажите папку с документацией.");
      return;
    }
    if (!docsUnderRepo) {
      setError("Папка документации должна находиться внутри репозитория.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onConfirm(docsRoot.trim());
    } catch (e) {
      setError(toMessage(e));
      setBusy(false);
      return;
    }
    if (addToGitignore) {
      try {
        // OptMem-aware block: `.atlas/*` ignored, `!.atlas/memory/**` kept trackable.
        await ensureAtlasGitignore(probe.root);
      } catch (e) {
        console.error("Failed to update .gitignore", e);
      }
    }
  };

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-labelledby="confirm-open-title"
        onClick={(event) => event.stopPropagation()}
        style={{ width: "min(520px, 100%)" }}
      >
        <div className="clone-modal-title" id="confirm-open-title">
          Укажите корень документации
        </div>

        <div className="clone-modal-message">
          Репозиторий: <b>{folderName(probe.root)}</b>
          <div style={{ marginTop: 6, fontSize: 10.5, color: "var(--text-2)", wordBreak: "break-all" }}>
            {probe.root}
          </div>
        </div>

        {offeredBranches.length > 1 ? (
          <div className="clone-modal-field">
            <span className="clone-modal-label" id="branches-label">
              Ветка
            </span>
            <div className="clone-select" ref={branchesRef}>
              <button
                type="button"
                className={`clone-select-trigger${branchesOpen ? " is-open" : ""}`}
                aria-haspopup="listbox"
                aria-expanded={branchesOpen}
                aria-labelledby="branches-label"
                disabled={switching || busy}
                onClick={() => setBranchesOpen((isOpen) => !isOpen)}
              >
                <span className="clone-select-value">
                  {currentBranch ? (
                    <span className="clone-select-path">{currentBranch}</span>
                  ) : (
                    // Unborn HEAD: a clone with no commit on the default
                    // branch has no current branch to show.
                    <span className="clone-select-placeholder">Выберите…</span>
                  )}
                  {switching ? (
                    <span className="clone-select-reason">Переключение…</span>
                  ) : null}
                </span>
                <span className="clone-select-chevron" aria-hidden>
                  ▾
                </span>
              </button>
              {branchesOpen ? (
                <div className="clone-select-menu" role="listbox">
                  {offeredBranches.map((b) => (
                    <button
                      key={`${b.isRemote ? "remote" : "local"}:${b.name}`}
                      type="button"
                      role="option"
                      aria-selected={b.isCurrent}
                      className={`clone-select-option${b.isCurrent ? " is-active" : ""}`}
                      onClick={() => {
                        setBranchesOpen(false);
                        void switchBranch(b.name);
                      }}
                    >
                      <span className="clone-select-path">{localBranchName(b)}</span>
                      <span className="clone-select-reason">
                        {b.isCurrent
                          ? "текущая"
                          : b.isRemote
                            ? "удалённая"
                            : "локальная"}
                      </span>
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
            {candidates.length === 0 ? (
              <span className="clone-modal-hint">
                В этой ветке документация не найдена. Если репозиторий новый,
                шаблон может лежать в другой ветке — выберите её здесь.
              </span>
            ) : null}
          </div>
        ) : null}

        {candidates.length > 0 ? (
          <div className="clone-modal-field">
            <span className="clone-modal-label" id="candidates-label">
              Найденные варианты
            </span>
            <div className="clone-select" ref={candidatesRef}>
              <button
                type="button"
                className={`clone-select-trigger${candidatesOpen ? " is-open" : ""}`}
                aria-haspopup="listbox"
                aria-expanded={candidatesOpen}
                aria-labelledby="candidates-label"
                onClick={() => setCandidatesOpen((isOpen) => !isOpen)}
              >
                <span className="clone-select-value">
                  {selectedCandidate ? (
                    <>
                      <span className="clone-select-path">
                        {selectedCandidate.relativePath}
                      </span>
                      <span className="clone-select-reason">
                        {selectedCandidate.reason}
                      </span>
                    </>
                  ) : (
                    <span className="clone-select-placeholder">Выберите…</span>
                  )}
                </span>
                <span className="clone-select-chevron" aria-hidden>
                  ▾
                </span>
              </button>
              {candidatesOpen ? (
                <div className="clone-select-menu" role="listbox">
                  {candidates.map((c) => {
                    const active = c.path === docsRoot;
                    return (
                      <button
                        key={c.path}
                        type="button"
                        role="option"
                        aria-selected={active}
                        className={`clone-select-option${active ? " is-active" : ""}`}
                        onClick={() => {
                          setDocsRoot(c.path);
                          setCandidatesOpen(false);
                          setError(null);
                        }}
                      >
                        <span className="clone-select-path">{c.relativePath}</span>
                        <span className="clone-select-reason">{c.reason}</span>
                      </button>
                    );
                  })}
                </div>
              ) : null}
            </div>
          </div>
        ) : (
          <div className="clone-modal-message">
            Автоматически найти корень документации не удалось. 
            <br />
            Укажите папку с документацией проекта самостоятельно.
          </div>
        )}

        <label className="clone-modal-field">
          <span className="clone-modal-label">Корень документации</span>
          <div className="clone-modal-path-row">
            <input
              className="clone-modal-input"
              type="text"
              value={docsRoot}
              onChange={(event) => setDocsRoot(event.target.value)}
            />
            <button
              type="button"
              className="clone-modal-browse"
              onClick={() => void pickDocsFolder()}
            >
              Обзор…
            </button>
          </div>
        </label>

        <label className="clone-modal-checkbox-label">
          <input
            type="checkbox"
            checked={addToGitignore}
            onChange={(event) => setAddToGitignore(event.target.checked)}
            className="clone-modal-checkbox"
          />
          <span>Добавить файлы настроек приложения в .gitignore</span>
        </label>

        {error ? <div className="clone-modal-message">{error}</div> : null}

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onCancel}>
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={() => void submit()}
            disabled={busy || !docsRoot.trim()}
          >
            Открыть
          </button>
        </div>
      </div>
    </div>
  );
}
