import { useEffect, useRef, useState } from "react";
import { FileInput } from "lucide-react";
import { ARTIFACT_KIND_LABELS, artifactCreateDraft, type ArtifactContent, type ArtifactKind } from "../../lib/artifacts";
import { toMessage } from "../../lib/errors";
import type { ToolCallBlock } from "../../lib/chatBlocks";

type RequestArgs = {
  kind: ArtifactKind;
  title: string;
  purpose: string;
  prefill: ArtifactContent | null;
};

type AssistantArtifactCardProps = {
  blocks: ToolCallBlock[];
  /** Resolves the pause with the artifact the user finished. */
  onAnswer: (id: string, artifactId: string) => void;
  /** «Заполню позже» — resolves the pause without an artifact. */
  onDefer: (id: string) => void;
  chatId: string | null;
};

/** Detail of the `atlas-artifact-ready` event the builder dispatches. The
 * `handled` flag is written back by whichever card owns that artifact,
 * synchronously during dispatch — that is how the builder learns whether a
 * paused turn consumed it, and therefore whether it must fall back to
 * sending a chat message instead. */
export type ArtifactReadyDetail = { artifactId: string; handled: boolean };

function parseRequestArgs(argumentsJson: string): RequestArgs | null {
  try {
    const parsed = JSON.parse(argumentsJson) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") return null;
    if (parsed.kind !== "httpRequest") return null;
    if (typeof parsed.title !== "string" || typeof parsed.purpose !== "string") return null;
    const prefill =
      parsed.prefill && typeof parsed.prefill === "object"
        ? ({ ...(parsed.prefill as object), kind: parsed.kind } as ArtifactContent)
        : null;
    return { kind: parsed.kind, title: parsed.title, purpose: parsed.purpose, prefill };
  } catch {
    return null;
  }
}

/** Mid-turn card for a pending `requestArtifact` call. Unlike an approval
 * card there is no countdown: the turn stays paused while the user fills
 * the builder in another tab, for as long as that takes (the pause is
 * persisted, so it even survives closing the app).
 *
 * Opening the builder does *not* resolve the pause — the card stays put,
 * showing that the assistant is still waiting. It is resolved either from
 * the builder's «Отправить ассистенту» button or by «Заполню позже» here.
 *
 * Known limitation: the draft id lives in this component, not in the
 * persisted block, so a full app restart while the pause is open loses the
 * *correlation* (not the artifact — it is in the store, and the pause is
 * still answerable). «Открыть конструктор» would then mint a second draft.
 * The recovery path is Инструменты → Артефакты: reopen the original and
 * press «Отправить ассистенту», which falls back to announcing it in chat
 * for the assistant to read with the `artifact` tool. */
export function AssistantArtifactCard({
  blocks,
  onAnswer,
  onDefer,
  chatId,
}: AssistantArtifactCardProps) {
  const [opening, setOpening] = useState(false);
  const [opened, setOpened] = useState(false);
  const [deferred, setDeferred] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The artifact this card created, if any. Correlating the builder's
  // «Отправить ассистенту» back to the right paused call happens here
  // rather than in the tab layer: this component is the only place that
  // knows both the tool-call id and the artifact id it minted for it.
  const createdIdRef = useRef<string | null>(null);
  const answeredRef = useRef(false);

  // A round with several artifact requests is not something the tool
  // description invites, but grouping mirrors the ask card and costs
  // nothing; the first block drives the card's copy.
  const primary = blocks[0];
  const args = primary ? parseRequestArgs(primary.argumentsJson) : null;

  const handleOpen = async () => {
    if (!args || opening || deferred) return;
    setOpening(true);
    setError(null);
    try {
      const record = await artifactCreateDraft({
        kind: args.kind,
        title: args.title,
        purpose: args.purpose,
        prefill: args.prefill,
        chatId,
      });
      createdIdRef.current = record.id;
      setOpened(true);
      window.dispatchEvent(
        new CustomEvent("atlas-open-artifact", { detail: { artifactId: record.id } }),
      );
    } catch (e) {
      setError(toMessage(e));
    } finally {
      setOpening(false);
    }
  };

  useEffect(() => {
    const onReady = (event: Event) => {
      const detail = (event as CustomEvent<ArtifactReadyDetail>).detail;
      const created = createdIdRef.current;
      if (!detail || answeredRef.current || !created || detail.artifactId !== created) return;
      answeredRef.current = true;
      detail.handled = true;
      for (const block of blocks) onAnswer(block.id, created);
    };
    window.addEventListener("atlas-artifact-ready", onReady);
    return () => window.removeEventListener("atlas-artifact-ready", onReady);
  }, [blocks, onAnswer]);

  const handleDefer = () => {
    if (deferred) return;
    setDeferred(true);
    for (const block of blocks) onDefer(block.id);
  };

  if (!args) {
    return (
      <div className="assistant-artifact-card">
        <p className="assistant-artifact-card-error">Не удалось разобрать запрос артефакта.</p>
        <div className="assistant-artifact-card-actions">
          <button type="button" className="assistant-btn" onClick={handleDefer} disabled={deferred}>
            Пропустить
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`assistant-artifact-card${deferred ? " is-decided" : ""}`}>
      <div className="assistant-artifact-card-header">
        <span className="assistant-artifact-card-icon">
          <FileInput size={14} strokeWidth={1.75} aria-hidden />
        </span>
        <div className="assistant-artifact-card-heading">
          <span className="assistant-artifact-card-eyebrow">
            Ассистент просит собрать артефакт · {ARTIFACT_KIND_LABELS[args.kind]}
          </span>
          <div className="assistant-artifact-card-title">{args.title}</div>
        </div>
      </div>

      <p className="assistant-artifact-card-purpose">{args.purpose}</p>

      {error ? <p className="assistant-artifact-card-error">{error}</p> : null}

      {opened ? (
        <p className="assistant-artifact-card-hint">
          Конструктор открыт во вкладке. Заполните его и нажмите «Отправить ассистенту» — ход
          продолжится сам.
        </p>
      ) : null}

      <div className="assistant-artifact-card-actions">
        <button type="button" className="assistant-btn" onClick={handleDefer} disabled={deferred}>
          Заполню позже
        </button>
        <button
          type="button"
          className="assistant-btn primary"
          onClick={() => void handleOpen()}
          disabled={opening || deferred}
        >
          {opened ? "Открыть снова" : "Открыть конструктор"}
        </button>
      </div>
    </div>
  );
}
