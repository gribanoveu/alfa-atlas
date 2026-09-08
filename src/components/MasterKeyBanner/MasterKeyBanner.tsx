import { ShieldAlert } from "lucide-react";
import { useMasterKeyAccess } from "../../hooks/useMasterKeyAccess";
import "./MasterKeyBanner.css";

/** Warns that every stored password and token is unreadable until the OS
 * grants access to the master key, and offers the retry.
 *
 * Rendered both in the app shell and inside the Settings dialog on purpose:
 * Settings is a full-window backdrop, so the shell's copy is hidden exactly
 * when someone goes looking for the credentials that appear to be missing. */
export function MasterKeyBanner() {
  const { unreachable, retryBusy, retryDenied, retry } = useMasterKeyAccess();

  if (!unreachable) return null;

  return (
    <div className="master-key-banner" role="alert">
      <ShieldAlert size={15} className="master-key-banner-icon" aria-hidden />
      <div className="master-key-banner-text">
        <span className="master-key-banner-title">Нет доступа к ключу шифрования</span>
        <span className="master-key-banner-detail">
          {retryDenied
            ? "Система снова отказала. Разрешите доступ в системном хранилище ключей и повторите."
            : "Сохранённые пароли и токены недоступны, пока система не разрешит доступ."}
        </span>
      </div>
      <button
        type="button"
        className="master-key-banner-btn"
        disabled={retryBusy}
        onClick={() => void retry()}
      >
        {retryBusy ? "Запрос…" : "Запросить доступ"}
      </button>
    </div>
  );
}
