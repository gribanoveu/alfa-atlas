import { Ticket } from "lucide-react";

import type { ToolCallBlock } from "../../lib/chatBlocks";
import { ARTIFACT_KIND_LABELS } from "../../lib/artifacts";
import { AssistantLoadingBars } from "./AssistantLoadingBars";
import "./AssistantTicketCard.css";

/** Only the write ops get a card. `list`/`read` are the assistant consulting
 *  something that already exists — there is nothing new for the user to open,
 *  and a card per read would bury the conversation. */
export function isTicketToolBlock(block: ToolCallBlock): boolean {
  if (block.name !== "artifact") return false;
  try {
    const op = (JSON.parse(block.argumentsJson) as { op?: string }).op;
    return op === "create" || op === "update";
  } catch {
    return false;
  }
}

type CardFacts = {
  op: "create" | "update";
  /** Present as soon as the call is made; the result carries the real one. */
  title: string;
};

function factsFromArgs(block: ToolCallBlock): CardFacts {
  try {
    const args = JSON.parse(block.argumentsJson) as { op?: string; title?: string };
    return {
      op: args.op === "update" ? "update" : "create",
      title: args.title?.trim() || "Тикет",
    };
  } catch {
    return { op: "create", title: "Тикет" };
  }
}

type AssistantTicketCardProps = {
  block: ToolCallBlock;
  onOpenArtifact: (artifactId: string) => void;
};

/** The chat's handle on an artifact the assistant wrote itself.
 *
 *  Without it a created ticket is invisible: unlike `requestArtifact` — which
 *  pauses the turn behind a card the user must answer — a write op settles
 *  silently, and the only other way to reach the result would be the
 *  artifacts dialog. */
export function AssistantTicketCard({ block, onOpenArtifact }: AssistantTicketCardProps) {
  const { op } = factsFromArgs(block);
  const settled = block.result?.tool === "artifact" ? block.result.result : null;

  if (block.status === "error") {
    return (
      <div className="ticket-card is-error">
        <span className="ticket-card-eyebrow">Не удалось сохранить тикет</span>
        {block.errorMessage ? (
          <p className="ticket-card-message">{block.errorMessage}</p>
        ) : null}
      </div>
    );
  }

  if (!settled) {
    return (
      <div className="ticket-card">
        <span className="ticket-card-eyebrow">
          {op === "update" ? "Правит тикет" : "Составляет тикет"}
        </span>
        <AssistantLoadingBars />
      </div>
    );
  }

  const { artifact } = settled;
  const subtitle =
    artifact.content.kind === "jiraTicket"
      ? firstLine(artifact.content.outcome) || firstLine(artifact.content.why)
      : "";

  return (
    <div className="ticket-card">
      <span className="ticket-card-eyebrow">
        {ARTIFACT_KIND_LABELS[artifact.kind]} · {op === "update" ? "обновлён" : "готов"}
      </span>
      <button
        type="button"
        className="ticket-card-open"
        onClick={() => onOpenArtifact(artifact.id)}
        title="Открыть во вкладке"
      >
        <Ticket size={14} className="ticket-card-icon" aria-hidden />
        <span className="ticket-card-text">
          <span className="ticket-card-title">{artifact.title}</span>
          {subtitle ? <span className="ticket-card-subtitle">{subtitle}</span> : null}
        </span>
      </button>
    </div>
  );
}

function firstLine(text: string): string {
  return text.split("\n").map((line) => line.trim()).find(Boolean) ?? "";
}
