import "./StatusBar.css";

type StatusBarProps = {
  filePath: string;
  formatLabel: string;
  lineEndingLabel: string;
  cursorLabel: string;
  hasActiveFile: boolean;
};

export function StatusBar({
  filePath,
  formatLabel,
  lineEndingLabel,
  cursorLabel,
  hasActiveFile,
}: StatusBarProps) {
  return (
    <footer className="statusbar">
      <div className="seg" title={filePath}>
        {filePath}
      </div>
      <div className="grow" />
      <div className="seg ai">AI-индекс актуален</div>
      {hasActiveFile ? (
        <>
          <div className="seg" title="Формат файла">
            {formatLabel}
          </div>
          <div className="seg" title="Окончания строк">
            {lineEndingLabel}
          </div>
          <div className="seg" title="Позиция курсора">
            {cursorLabel}
          </div>
        </>
      ) : (
        <>
          <div className="seg">—</div>
          <div className="seg">{cursorLabel}</div>
        </>
      )}
    </footer>
  );
}
