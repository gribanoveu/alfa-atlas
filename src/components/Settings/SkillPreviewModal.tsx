import { useEffect, useState } from "react";
import { useSkillPreview } from "../../hooks/useSkillPreview";
import { extensionOf, isMarkdownPath } from "../../lib/fileExtensions";
import { splitSkillFrontmatter, type SkillListItem } from "../../lib/skills";
import { AssistantMarkdown } from "../RightDock/AssistantMarkdown";
import "../ToolLog/ToolCallLogModal.css";
import "./SkillPreviewModal.css";

type SkillPreviewModalProps = {
  skill: SkillListItem;
  onClose: () => void;
};

/** Wraps `text` in a fenced code block long enough to survive backticks in
 * the file itself, so it can go through the same Markdown renderer. */
function asCodeFence(text: string, lang: string): string {
  const longestRun = Math.max(0, ...Array.from(text.matchAll(/`+/g), (m) => m[0].length));
  const fence = "`".repeat(Math.max(3, longestRun + 1));
  return `${fence}${lang}\n${text}\n${fence}`;
}

/** Fenced-code language tag for a companion file, from its extension. Shiki
 * falls back to plain text for anything it doesn't know (`.adoc`). */
function fenceLang(path: string): string {
  return extensionOf(path).replace(/^\./, "");
}

type SkillFileViewProps = {
  path: string;
  content: string;
  rendered: boolean;
};

function SkillFileView({ path, content, rendered }: SkillFileViewProps) {
  if (!rendered || !isMarkdownPath(path)) {
    return <AssistantMarkdown content={asCodeFence(content, fenceLang(path))} streaming={false} />;
  }
  // Frontmatter is metadata, not prose: shown as-is above the body instead of
  // being handed to Markdown, which would turn it into a heading.
  const { frontmatter, body } = splitSkillFrontmatter(content);
  return (
    <>
      {frontmatter ? <pre className="skill-preview-frontmatter">{frontmatter}</pre> : null}
      <AssistantMarkdown content={body} streaming={false} />
    </>
  );
}

/** Read-only viewer for one skill: its files on the left, the selected file
 * rendered on the right. Editing lives outside the app — the folder button in
 * the Settings tab opens it in the OS file manager. */
export function SkillPreviewModal({ skill, onClose }: SkillPreviewModalProps) {
  const { files, selected, select, content, loadingContent, error } = useSkillPreview(
    skill.source,
    skill.name,
  );
  const [rendered, setRendered] = useState(true);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const isMarkdown = selected !== null && isMarkdownPath(selected);

  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog skill-preview-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="skill-preview-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="tool-log-header">
          <h2 className="tool-log-title" id="skill-preview-title">
            {skill.name}
            <span className="skill-preview-source">
              {skill.source === "bundled" ? "встроенный" : "пользовательский"} · только просмотр
            </span>
          </h2>
          <div className="tool-log-header-actions">
            {isMarkdown ? (
              <div className="skill-preview-modes" role="group" aria-label="Режим просмотра">
                <button
                  type="button"
                  className={`tool-log-btn${rendered ? " is-active" : ""}`}
                  aria-pressed={rendered}
                  onClick={() => setRendered(true)}
                >
                  Разметка
                </button>
                <button
                  type="button"
                  className={`tool-log-btn${rendered ? "" : " is-active"}`}
                  aria-pressed={!rendered}
                  onClick={() => setRendered(false)}
                >
                  Исходник
                </button>
              </div>
            ) : null}
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>

        {error ? <div className="tool-log-error">{error}</div> : null}

        <div className="skill-preview-layout">
          <aside className="skill-preview-files">
            {files === null ? (
              <div className="tool-log-empty">Загрузка…</div>
            ) : files.length === 0 ? (
              <div className="tool-log-empty">Нет файлов</div>
            ) : (
              <ul className="skill-preview-file-list">
                {files.map((path) => (
                  <li key={path}>
                    <button
                      type="button"
                      className={`skill-preview-file${selected === path ? " is-active" : ""}`}
                      title={path}
                      onClick={() => select(path)}
                    >
                      {path}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </aside>

          <section className="skill-preview-content">
            {selected === null ? (
              <div className="tool-log-empty">Выберите файл слева</div>
            ) : loadingContent ? (
              <div className="tool-log-empty">Загрузка…</div>
            ) : content === null ? (
              <div className="tool-log-empty">Файл не удалось прочитать</div>
            ) : (
              <div className="skill-preview-scroll">
                <SkillFileView path={selected} content={content} rendered={rendered} />
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
