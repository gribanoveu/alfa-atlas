import { findUtility, type UtilityId } from "../../data/utilities";
import { HttpStatusReference } from "./HttpStatusReference";
import { IdGenerator } from "./IdGenerator";
import { JwtParser } from "./JwtParser";
import { UnixtimeConverter } from "./UnixtimeConverter";
import "./UtilityView.css";

type UtilityViewProps = {
  utilityId: UtilityId;
};

/** Содержимое вкладки утилиты. Сами утилиты пока не реализованы — вкладка
 *  открывается заглушкой; реализация подставляется здесь по `utilityId`. */
export function UtilityView({ utilityId }: UtilityViewProps) {
  const utility = findUtility(utilityId);
  if (!utility) {
    return <div className="utility-view-empty">Утилита не найдена</div>;
  }

  const Icon = utility.icon;
  return (
    <div className="utility-view">
      <header className="utility-view-head">
        <div className="utility-view-icon">
          <Icon size={18} strokeWidth={1.75} aria-hidden />
        </div>
        <div className="utility-view-heading">
          <h1 className="utility-view-title">{utility.title}</h1>
          <p className="utility-view-desc">{utility.description}</p>
        </div>
      </header>
      {utilityId === "unixtime" ? <UnixtimeConverter /> : null}
      {utilityId === "ids" ? <IdGenerator /> : null}
      {utilityId === "jwt" ? <JwtParser /> : null}
      {utilityId === "http-status" ? <HttpStatusReference /> : null}
      {utility.stub ? (
        <div className="utility-view-stub" role="status">
          Утилита ещё не реализована
        </div>
      ) : null}
    </div>
  );
}
