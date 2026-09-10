import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Loader2, Save, Send } from "lucide-react";
import type { ArtifactReadyDetail } from "../RightDock/AssistantArtifactCard";
import {
  ARTIFACT_KIND_LABELS,
  ARTIFACT_UPDATED_EVENT,
  artifactGet,
  decideArtifactUpdate,
  artifactSave,
  type ArtifactContent,
  type ArtifactRecord,
  type ArtifactUpdatedDetail,
} from "../../lib/artifacts";
import { toMessage } from "../../lib/errors";
import { HttpRequestBuilder } from "./HttpRequestBuilder";
import { JiraTicketBuilder } from "./JiraTicketBuilder";
import "./ArtifactView.css";

type ArtifactViewProps = {
  artifactId: string;
  /** Surfaces unsaved changes on the tab strip, same as a file tab. */
  onDirtyChange: (artifactId: string, dirty: boolean) => void;
  onTitleChange: (artifactId: string, title: string) => void;
  /** Fallback when no paused turn consumed the finished artifact — drops a
   *  message into the chat naming it, so the assistant can read it with the
   *  `artifact` tool. */
  onSendToAssistant: (record: ArtifactRecord) => void;
};

/** One artifact's builder tab: a shared header (title, purpose, save/send)
 *  plus the per-kind editor, dispatched the way `UtilityView` dispatches on
 *  `utilityId`. */
export function ArtifactView({
  artifactId,
  onDirtyChange,
  onTitleChange,
  onSendToAssistant,
}: ArtifactViewProps) {
  const [record, setRecord] = useState<ArtifactRecord | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const [sent, setSent] = useState(false);
  // Версия ассистента, придержанная из-за несохранённых правок пользователя.
  const [agentUpdate, setAgentUpdate] = useState<ArtifactRecord | null>(null);

  useEffect(() => {
    let cancelled = false;
    setRecord(null);
    setLoadError(null);
    void (async () => {
      try {
        const loaded = await artifactGet(artifactId);
        if (cancelled) return;
        setRecord(loaded);
        setSent(loaded.status === "ready");
        onTitleChange(artifactId, loaded.title);
      } catch (e) {
        if (!cancelled) setLoadError(toMessage(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [artifactId, onTitleChange]);

  useEffect(() => {
    onDirtyChange(artifactId, dirty);
  }, [artifactId, dirty, onDirtyChange]);

  // Unmounting with unsaved edits would silently lose them, and this tab
  // has no autosave (unlike a file tab, there is no on-disk file to write
  // through to until the user commits).
  useEffect(() => {
    return () => onDirtyChange(artifactId, false);
  }, [artifactId, onDirtyChange]);

  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    return () => {
      if (savedTimerRef.current !== null) clearTimeout(savedTimerRef.current);
    };
  }, []);

  // Слушатель живёт весь срок вкладки, а `dirty`/`record` меняются на каждый
  // ввод — держим их в ref, чтобы не переподписываться на каждое нажатие.
  const dirtyRef = useRef(dirty);
  const recordRef = useRef<ArtifactRecord | null>(record);
  useEffect(() => {
    dirtyRef.current = dirty;
    recordRef.current = record;
  }, [dirty, record]);

  const adoptRecord = useCallback(
    (incoming: ArtifactRecord) => {
      setRecord(incoming);
      setDirty(false);
      setJustSaved(false);
      setSent(incoming.status === "ready");
      setAgentUpdate(null);
      onTitleChange(incoming.id, incoming.title);
    },
    [onTitleChange],
  );

  // Ассистент правит артефакт своим инструментом — открытая вкладка должна
  // показать это сразу, а не после закрытия и повторного открытия.
  useEffect(() => {
    const onUpdated = (event: Event) => {
      const incoming = (event as CustomEvent<ArtifactUpdatedDetail>).detail?.artifact;
      if (!incoming || incoming.id !== artifactId) return;
      switch (decideArtifactUpdate(incoming, recordRef.current, dirtyRef.current)) {
        case "adopt":
          adoptRecord(incoming);
          break;
        case "hold":
          setAgentUpdate(incoming);
          break;
        case "ignore":
          break;
      }
    };
    window.addEventListener(ARTIFACT_UPDATED_EVENT, onUpdated);
    return () => window.removeEventListener(ARTIFACT_UPDATED_EVENT, onUpdated);
  }, [artifactId, adoptRecord]);

  const updateContent = useCallback((content: ArtifactContent) => {
    setRecord((prev) => (prev ? { ...prev, content } : prev));
    setDirty(true);
    setJustSaved(false);
  }, []);

  const updateTitle = useCallback(
    (title: string) => {
      setRecord((prev) => (prev ? { ...prev, title } : prev));
      setDirty(true);
      setJustSaved(false);
      onTitleChange(artifactId, title);
    },
    [artifactId, onTitleChange],
  );

  const persist = useCallback(
    async (status: ArtifactRecord["status"]): Promise<ArtifactRecord | null> => {
      if (!record) return null;
      setBusy(true);
      setSaveError(null);
      try {
        const saved = await artifactSave({ ...record, status });
        setRecord(saved);
        setDirty(false);
        return saved;
      } catch (e) {
        setSaveError(toMessage(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    [record],
  );

  const handleSave = async () => {
    // Saving must not silently promote a draft to ready — that is what the
    // send button is for.
    const saved = await persist(record?.status ?? "draft");
    if (!saved) return;
    setJustSaved(true);
    if (savedTimerRef.current !== null) clearTimeout(savedTimerRef.current);
    savedTimerRef.current = setTimeout(() => setJustSaved(false), 2000);
  };

  const handleSend = async () => {
    const saved = await persist("ready");
    if (!saved) return;
    setSent(true);
    // A card in the chat claims this synchronously if a turn is paused on
    // it; otherwise nothing does, and the artifact has to announce itself.
    const detail: ArtifactReadyDetail = { artifactId: saved.id, handled: false };
    window.dispatchEvent(new CustomEvent("atlas-artifact-ready", { detail }));
    if (!detail.handled) onSendToAssistant(saved);
  };

  if (loadError) {
    return <div className="artifact-view-empty">Не удалось открыть артефакт: {loadError}</div>;
  }
  if (!record) {
    return (
      <div className="artifact-view-empty">
        <Loader2 size={16} className="artifact-view-spinner" aria-hidden /> Загрузка…
      </div>
    );
  }

  return (
    <div className="artifact-view">
      <header className="artifact-view-head">
        <div className="artifact-view-heading">
          <span className="artifact-view-eyebrow">
            Артефакт · {ARTIFACT_KIND_LABELS[record.kind]}
            {sent && !dirty
              ? " · отправлен ассистенту"
              : record.status === "draft"
                ? " · черновик"
                : ""}
          </span>
          <input
            className="artifact-view-title"
            value={record.title}
            aria-label="Название артефакта"
            onChange={(e) => updateTitle(e.target.value)}
          />
        </div>
        <div className="artifact-view-actions">
          <button type="button" className="artifact-btn" disabled={busy || !dirty} onClick={() => void handleSave()}>
            {justSaved ? <Check size={13} aria-hidden /> : <Save size={13} aria-hidden />}
            {justSaved ? "Сохранено" : "Сохранить"}
          </button>

          {/* Every kind now goes back to the assistant. A ticket used to be
              the exception — it was written *by* the model and its way out
              was into the tracker — but with publishing gone the send button
              is again how the user asks for a refinement. */}
          <button type="button" className="artifact-btn primary" disabled={busy} onClick={() => void handleSend()}>
            <Send size={13} aria-hidden />
            Отправить ассистенту
          </button>
        </div>
      </header>

      {record.purpose ? (
        <p className="artifact-view-purpose">
          <span className="artifact-view-purpose-label">Зачем это нужно ассистенту:</span> {record.purpose}
        </p>
      ) : null}

      {agentUpdate ? (
        <div className="artifact-agent-update">
          <span>
            Ассистент изменил артефакт, пока вы его правили. Ваши изменения не сохранены —
            выберите, чью версию оставить.
          </span>
          <div className="artifact-agent-update-actions">
            <button
              type="button"
              className="artifact-btn"
              onClick={() => setAgentUpdate(null)}
            >
              Оставить свою
            </button>
            <button
              type="button"
              className="artifact-btn primary"
              onClick={() => adoptRecord(agentUpdate)}
            >
              Показать версию ассистента
            </button>
          </div>
        </div>
      ) : null}

      {saveError ? <p className="artifact-view-error">{saveError}</p> : null}

      {record.content.kind === "httpRequest" ? (
        <HttpRequestBuilder spec={record.content} onChange={updateContent} />
      ) : record.content.kind === "jiraTicket" ? (
        <JiraTicketBuilder spec={record.content} onChange={updateContent} />
      ) : null}
    </div>
  );
}
