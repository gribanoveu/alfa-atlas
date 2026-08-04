import { Check, Download, RefreshCw, Settings2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEmbeddingSetup } from "../../hooks/useEmbeddingSetup";
import "./AssistantPanel.css";

type AssistantPanelProps = {
  onOpenSettings: () => void;
};

type ChecklistItem = {
  id: string;
  Icon: LucideIcon;
  title: string;
  description: string;
  completed: boolean;
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
  secondaryLabel?: string;
  onSecondaryAction?: () => void;
};

export function AssistantPanel({ onOpenSettings }: AssistantPanelProps) {
  const {
    config,
    modelStatus,
    providerConfigured,
    lastSync,
    busy,
    downloadModel,
    cancelDownload,
    sync,
  } = useEmbeddingSetup();

  const items: ChecklistItem[] = [];

  items.push({
    id: "provider",
    Icon: Settings2,
    title: "Настроить провайдера",
    description:
      config?.kind === "remote"
        ? "Внешний API — укажите base URL, модель и API ключ в настройках."
        : "Локальная модель BGE-M3 — выполняется на устройстве.",
    completed: providerConfigured,
    actionLabel: "Открыть настройки",
    onAction: onOpenSettings,
  });

  if (config?.kind === "local") {
    items.push({
      id: "model",
      Icon: Download,
      title: "Загрузить модель",
      description:
        modelStatus.status === "downloading"
          ? `Загрузка… ${Math.round(modelStatus.progress * 100)}%`
          : modelStatus.status === "error"
            ? `Ошибка: ${modelStatus.message}`
            : "BGE-M3, int8 ONNX, ~570 МБ. Загружается один раз и кэшируется локально.",
      completed: modelStatus.status === "ready",
      actionLabel: modelStatus.status === "downloading" ? "Загрузка…" : "Скачать",
      onAction: () => void downloadModel(),
      actionDisabled: busy || modelStatus.status === "downloading",
      secondaryLabel: modelStatus.status === "downloading" ? "Отменить" : undefined,
      onSecondaryAction:
        modelStatus.status === "downloading" ? () => void cancelDownload() : undefined,
    });
  }

  items.push({
    id: "sync",
    Icon: RefreshCw,
    title: "Синхронизировать индекс",
    description: lastSync
      ? `Добавлено ${lastSync.embedded}, без изменений ${lastSync.skippedUnchanged}, удалено ${lastSync.removed}.`
      : "Построить/обновить эмбеддинги чанков документации для текущего проекта.",
    completed: lastSync !== null,
    actionLabel: "Синхронизировать",
    onAction: () => void sync(),
    actionDisabled: busy || !providerConfigured,
  });

  return (
    <div className="assistant-panel">
      <section className="assistant-panel-section">
        <h3 className="assistant-panel-section-title">Настройка эмбеддингов</h3>
        <div className="assistant-checklist">
          {items.map((item) => (
            <div
              key={item.id}
              className={`assistant-checklist-item ${item.completed ? "is-completed" : ""}`}
            >
              <div className="assistant-checklist-icon">
                <item.Icon size={14} strokeWidth={1.75} aria-hidden />
              </div>
              <div className="assistant-checklist-body">
                <span className="assistant-checklist-title">{item.title}</span>
                <span className="assistant-checklist-desc">{item.description}</span>
                {item.completed ? (
                  <span className="assistant-checklist-done">
                    <Check size={11} strokeWidth={2} aria-hidden />
                    Готово
                  </span>
                ) : (
                  <div className="assistant-checklist-actions">
                    <button
                      type="button"
                      className="assistant-checklist-btn primary"
                      disabled={item.actionDisabled}
                      onClick={item.onAction}
                    >
                      {item.actionLabel}
                    </button>
                    {item.secondaryLabel ? (
                      <button
                        type="button"
                        className="assistant-checklist-btn"
                        onClick={item.onSecondaryAction}
                      >
                        {item.secondaryLabel}
                      </button>
                    ) : null}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
