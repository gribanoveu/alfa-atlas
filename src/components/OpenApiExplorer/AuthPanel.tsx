import { useState } from "react";
import { Lock, LockOpen } from "lucide-react";
import {
  describeScheme,
  emptyValueFor,
  isFilled,
  type AuthValue,
  type AuthValues,
  type SecurityScheme,
} from "./security";
import "./OpenApiExplorer.css";

type AuthPanelProps = {
  schemes: SecurityScheme[];
  values: AuthValues;
  onChange: (schemeId: string, value: AuthValue) => void;
  onClear: () => void;
  open: boolean;
  onToggle: () => void;
  /** Схемы текущей операции — помечаем их, чтобы в длинном списке было видно,
   * какое поле влияет на кнопку «Выполнить» прямо сейчас. */
  activeSchemeIds: string[];
};

/** Аналог кнопки Authorize в Swagger UI: одни и те же секреты на всю
 * спецификацию, а не на конкретную операцию, — иначе токен приходится
 * вводить заново после каждого переключения ручки. Значения живут в стейте
 * вкладки: на диск не пишутся и после закрытия проекта не восстанавливаются. */
export function AuthPanel({
  schemes,
  values,
  onChange,
  onClear,
  open,
  onToggle,
  activeSchemeIds,
}: AuthPanelProps) {
  const [reveal, setReveal] = useState(false);

  if (schemes.length === 0) return null;

  const filled = schemes.filter((s) => isFilled(values[s.id]));
  const activeFilled = activeSchemeIds.filter((id) => isFilled(values[id]));
  const status =
    filled.length === 0
      ? "не заполнена"
      : activeSchemeIds.length > 0
        ? `${activeFilled.length} из ${activeSchemeIds.length} для этой операции`
        : `заполнено схем: ${filled.length}`;

  return (
    <div className="oas-auth">
      <div className="oas-auth-bar">
        <button
          type="button"
          className={`oas-auth-toggle${filled.length > 0 ? " is-filled" : ""}`}
          onClick={onToggle}
          aria-expanded={open}
        >
          {filled.length > 0 ? (
            <Lock size={13} aria-hidden />
          ) : (
            <LockOpen size={13} aria-hidden />
          )}
          Авторизация
        </button>
        <span className="oas-auth-status">{status}</span>
        {open ? (
          <label className="oas-auth-reveal">
            <input
              type="checkbox"
              checked={reveal}
              onChange={(e) => setReveal(e.target.checked)}
            />
            показать значения
          </label>
        ) : null}
        {filled.length > 0 ? (
          <button type="button" className="oas-auth-clear" onClick={onClear}>
            Сбросить
          </button>
        ) : null}
      </div>

      {open ? (
        <div className="oas-auth-body">
          {schemes.map((scheme) => {
            const value = values[scheme.id] ?? emptyValueFor(scheme);
            const isActive = activeSchemeIds.includes(scheme.id);
            return (
              <div
                key={scheme.id}
                className={`oas-auth-scheme${isActive ? " is-active" : ""}`}
              >
                <div className="oas-auth-scheme-head">
                  <span className="oas-auth-scheme-id">{scheme.id}</span>
                  {isActive ? (
                    <span className="oas-auth-scheme-tag">эта операция</span>
                  ) : null}
                </div>
                <div className="oas-auth-scheme-meta">{describeScheme(scheme)}</div>
                {scheme.description ? (
                  <div className="oas-auth-scheme-desc">{scheme.description}</div>
                ) : null}

                {scheme.kind === "unsupported" ? null : scheme.kind === "basic" ? (
                  <div className="oas-auth-fields">
                    <input
                      type="text"
                      className="oas-try-input"
                      placeholder="пользователь"
                      autoComplete="off"
                      value={value.kind === "basic" ? value.username : ""}
                      onChange={(e) =>
                        onChange(scheme.id, {
                          kind: "basic",
                          username: e.target.value,
                          password: value.kind === "basic" ? value.password : "",
                        })
                      }
                    />
                    <input
                      type={reveal ? "text" : "password"}
                      className="oas-try-input"
                      placeholder="пароль"
                      autoComplete="off"
                      value={value.kind === "basic" ? value.password : ""}
                      onChange={(e) =>
                        onChange(scheme.id, {
                          kind: "basic",
                          username: value.kind === "basic" ? value.username : "",
                          password: e.target.value,
                        })
                      }
                    />
                  </div>
                ) : (
                  <div className="oas-auth-fields">
                    <input
                      type={reveal ? "text" : "password"}
                      className="oas-try-input"
                      placeholder={
                        scheme.kind === "apiKey" ? (scheme.name ?? "значение ключа") : "токен"
                      }
                      autoComplete="off"
                      value={value.kind === "token" ? value.token : ""}
                      onChange={(e) =>
                        onChange(scheme.id, { kind: "token", token: e.target.value })
                      }
                    />
                  </div>
                )}
              </div>
            );
          })}
          <div className="oas-auth-note">
            Значения хранятся только в памяти этой вкладки и попадают в текст curl.
          </div>
        </div>
      ) : null}
    </div>
  );
}
