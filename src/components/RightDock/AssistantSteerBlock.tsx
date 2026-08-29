import { CornerDownRight } from "lucide-react";
import type { SteerBlock } from "../../lib/chatBlocks";
import "./AssistantSteerBlock.css";

type AssistantSteerBlockProps = {
  block: SteerBlock;
};

export function AssistantSteerBlock({ block }: AssistantSteerBlockProps) {
  return (
    <div className="assistant-steer-block">
      <CornerDownRight size={12} strokeWidth={1.75} aria-hidden />
      <span>
        <strong>Уточнение:</strong> {block.text}
      </span>
    </div>
  );
}
