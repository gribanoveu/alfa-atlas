import "./TopBar.css";

const MENU = [
  "Файл",
  "Правка",
  "Вид",
  "Навигация",
  "Git",
  "Инструменты",
  "Справка",
];

type TopBarProps = {
  repoName?: string;
  branchName?: string;
};

export function TopBar({ repoName = "—", branchName = "—" }: TopBarProps) {
  return (
    <header className="topbar">
      <div className="brand">
        <span className="dot" />
        docflow
      </div>
      <nav className="menu">
        {MENU.map((item) => (
          <button key={item} type="button" className="menu-item">
            {item}
          </button>
        ))}
      </nav>
      <div className="topbar-spacer" />
      <div className="topbar-right">
        <div className="repo-chip">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M9 2v20M15 2v6a3 3 0 0 1-3 3H6M9 8h.01" />
          </svg>
          <b>{repoName}</b>
        </div>
        <div className="branch-chip">⎇ {branchName}</div>
      </div>
    </header>
  );
}
