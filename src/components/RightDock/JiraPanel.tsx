import { AlertTriangle, CheckCircle2, RefreshCw, Settings2, Ticket } from "lucide-react";
import { useJiraConnection } from "../../hooks/useJiraConnection";
import { useJiraProject } from "../../hooks/useJiraProject";
import { JiraProjectPicker } from "../Jira/JiraProjectPicker";
import "./JiraPanel.css";

const MISSING_TEXT: Record<"instance" | "token", string> = {
  instance: "Не указан адрес Jira.",
  token: "Не сохранён токен доступа.",
};

export type JiraPanelProps = {
  onOpenSettings: () => void;
};

/** Reports whether the stored Jira token actually works, by showing the
 * account it belongs to. There is no separate "test" action: the identity
 * *is* the proof — it only appears when settings, token, TLS and the HTTP
 * round trip all worked. */
export function JiraPanel({ onOpenSettings }: JiraPanelProps) {
  const { state, refresh } = useJiraConnection();
  const project = useJiraProject();

  const busy = state.kind === "loading";
  // Одна строка под именем вместо подписанных полей: у Server всегда есть
  // логин, почта — не всегда, поэтому склеивается только то, что пришло.
  const meta =
    state.kind === "connected"
      ? [state.user.emailAddress, state.user.accountId].filter(Boolean).join(" · ")
      : "";

  return (
    <div className="jira-panel">
      <div className="jira-panel-toolbar">
        <span className="jira-panel-toolbar-title">Jira</span>
        <div className="jira-panel-toolbar-actions">
          <button
            type="button"
            className="jira-panel-icon-btn"
            disabled={busy}
            title="Проверить соединение"
            aria-label="Проверить соединение"
            onClick={() => void refresh()}
          >
            <RefreshCw className={busy ? "spin" : undefined} size={13} aria-hidden />
          </button>
          <button
            type="button"
            className="jira-panel-icon-btn"
            title="Настройки Jira"
            aria-label="Настройки Jira"
            onClick={onOpenSettings}
          >
            <Settings2 size={13} aria-hidden />
          </button>
        </div>
      </div>

      <div className="jira-panel-body">
        {state.kind === "idle" || state.kind === "loading" ? (
          <p className="jira-panel-status">Проверяем соединение…</p>
        ) : null}

        {state.kind === "unconfigured" ? (
          <div className="jira-panel-notice">
            <Ticket className="jira-panel-notice-icon" size={16} aria-hidden />
            <p className="jira-panel-notice-text">{MISSING_TEXT[state.missing]}</p>
            <button type="button" className="jira-panel-btn" onClick={onOpenSettings}>
              Настроить Jira
            </button>
          </div>
        ) : null}

        {state.kind === "error" ? (
          <div className="jira-panel-notice is-error">
            <AlertTriangle className="jira-panel-notice-icon" size={16} aria-hidden />
            <p className="jira-panel-notice-title">Соединение не работает</p>
            <p className="jira-panel-notice-text">{state.message}</p>
            <button type="button" className="jira-panel-btn" onClick={onOpenSettings}>
              Открыть настройки
            </button>
          </div>
        ) : null}

        {state.kind === "connected" ? (
          <div className="jira-panel-user">
            <div className="jira-panel-user-head">
              <div className="jira-panel-user-names">
                <span className="jira-panel-user-name" title={state.user.displayName}>
                  {state.user.displayName}
                </span>
                {meta ? (
                  <span className="jira-panel-user-meta" title={meta}>
                    {meta}
                  </span>
                ) : null}
              </div>

              {/* Только значок: подпись занимала половину строки, а сам
                  факт, что панель показывает имя, уже означает, что связь
                  есть. Текст остаётся в подсказке и для скринридера. */}
              <span
                className="jira-panel-connected"
                role="img"
                aria-label="Соединение установлено"
                title="Соединение установлено"
              >
                <CheckCircle2 size={14} aria-hidden />
              </span>
            </div>

            {!state.user.active ? (
              <p className="jira-panel-inactive">Пользователь отключён в Jira</p>
            ) : null}

            {/* The active project is switched here as well as in Settings:
                it changes with the task at hand, not with the setup, and
                sending someone into a settings dialog to do it would be the
                wrong weight for something this routine. */}
            {project.ready ? (
              <div className="jira-panel-project">
                <JiraProjectPicker
                  projectKey={project.projectKey}
                  projectName={project.projectName}
                  disabled={project.busy}
                  onPick={(picked) => void project.pick(picked)}
                />
                {project.error ? (
                  <p className="jira-panel-notice-text">{project.error}</p>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}
