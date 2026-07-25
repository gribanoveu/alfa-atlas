import Editor, { type OnMount } from "@monaco-editor/react";
import type { editor as MonacoEditor } from "monaco-editor";
import { useCallback } from "react";
import type {
  CursorPosition,
  EditorTab,
} from "../../hooks/useWorkspaceLayout";
import { EditorTabs } from "./EditorTabs";
import "./Editor.css";

type EditorPaneProps = {
  tabs: EditorTab[];
  activeTabId: string;
  activeTab: EditorTab;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onChangeContent: (content: string) => void;
  onCursorChange: (cursor: CursorPosition) => void;
};

export function EditorPane({
  tabs,
  activeTabId,
  activeTab,
  onSelectTab,
  onCloseTab,
  onChangeContent,
  onCursorChange,
}: EditorPaneProps) {
  const handleMount: OnMount = useCallback(
    (editor) => {
      const syncCursor = () => {
        const position = editor.getPosition();
        if (!position) return;
        onCursorChange({ line: position.lineNumber, column: position.column });
      };

      syncCursor();
      editor.onDidChangeCursorPosition(syncCursor);
    },
    [onCursorChange],
  );

  const handleChange = useCallback(
    (value: string | undefined) => {
      onChangeContent(value ?? "");
    },
    [onChangeContent],
  );

  const options: MonacoEditor.IStandaloneEditorConstructionOptions = {
    fontFamily: "'JetBrains Mono', ui-monospace, monospace",
    fontSize: 13,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    automaticLayout: true,
    padding: { top: 8 },
    renderLineHighlight: "line",
    wordWrap: "on",
  };

  return (
    <section className="editor-col">
      <EditorTabs
        tabs={tabs}
        activeTabId={activeTabId}
        onSelect={onSelectTab}
        onClose={onCloseTab}
      />
      <div className="editor-body">
        <Editor
          key={activeTab.id}
          height="100%"
          theme="vs-dark"
          language={activeTab.language}
          value={activeTab.content}
          onChange={handleChange}
          onMount={handleMount}
          options={options}
        />
      </div>
    </section>
  );
}
