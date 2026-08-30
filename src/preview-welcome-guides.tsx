import ReactDOM from "react-dom/client";
import { WelcomeGuides } from "./components/Welcome/WelcomeGuides";
import "./styles/tokens.css";
import "./styles/app.css";
import "./components/Welcome/Welcome.css";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
(window as any).__TAURI_INTERNALS__ = {
  transformCallback: (cb: unknown) => cb,
  invoke: async () => ({ completed: [] }),
};

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <section className="welcome">
    <div className="welcome-layout">
      <div className="welcome-inner">
        <header className="welcome-brand">
          <div className="welcome-brand-row">
            <span className="welcome-dot" />
            <h1 className="welcome-title">Alfa Atlas</h1>
          </div>
          <p className="welcome-subtitle">
            Добро пожаловать в редактор документации. Откройте папку проекта или
            склонируйте git-репозиторий, чтобы начать работу.
          </p>
        </header>
        <section className="welcome-section">
          <h2 className="welcome-section-title">Начало работы</h2>
          <div className="welcome-actions">
            <button type="button" className="welcome-action">
              <span className="welcome-action-label">Открыть папку…</span>
              <span className="welcome-action-hint">Выбрать локальный каталог</span>
            </button>
            <button type="button" className="welcome-action">
              <span className="welcome-action-label">Клонировать репозиторий…</span>
              <span className="welcome-action-hint">Склонировать и открыть</span>
            </button>
          </div>
        </section>
        <section className="welcome-section">
          <h2 className="welcome-section-title">Недавние</h2>
          <ul className="welcome-recent-list">
            <li className="welcome-recent-item">
              <button type="button" className="welcome-recent-open">
                <span className="welcome-recent-name">docflow</span>
                <span className="welcome-recent-path">/Users/eugene/Downloads/docflow</span>
              </button>
            </li>
          </ul>
        </section>
      </div>
      <aside className="welcome-side">
        <h2 className="welcome-section-title">Первые шаги</h2>
        <WelcomeGuides
          onOpenGitKey={() => console.log("git")}
          onOpenLlmKey={() => console.log("llm")}
        />
      </aside>
    </div>
  </section>,
);
