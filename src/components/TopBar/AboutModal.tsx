import { appConfig } from "../../lib/appConfig";
import "./AboutModal.css";

type AboutModalProps = {
  onClose: () => void;
};

export function AboutModal({ onClose }: AboutModalProps) {
  return (
    <div
      className="about-modal-backdrop"
      role="presentation"
      onClick={onClose}
    >
      <div
        className="about-modal"
        role="dialog"
        aria-labelledby="about-modal-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="about-modal-brand">
          <span className="about-modal-dot" />
          <h2 id="about-modal-title">Alfa Atlas</h2>
        </div>
        <p className="about-modal-version">Версия {appConfig.version}</p>
        <p className="about-modal-desc">
          Редактор документации для работы с git-репозиториями.
        </p>
        <div className="about-modal-actions">
          <button type="button" className="about-modal-btn" onClick={onClose}>
            Закрыть
          </button>
        </div>
      </div>
    </div>
  );
}
