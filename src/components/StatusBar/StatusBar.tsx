import "./StatusBar.css";

type StatusBarProps = {
  filePath: string;
  language: string;
  cursorLabel: string;
};

export function StatusBar({ filePath, language, cursorLabel }: StatusBarProps) {
  return (
    <footer className="statusbar">
      <div className="seg">{filePath}</div>
      <div className="grow" />
      <div className="seg ai">AI-индекс актуален</div>
      <div className="seg">{language}</div>
      <div className="seg">UTF-8</div>
      <div className="seg">{cursorLabel}</div>
    </footer>
  );
}
