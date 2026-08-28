import type { LucideIcon } from "lucide-react";
import { FileInput } from "lucide-react";
import type { ArtifactKind } from "../lib/artifacts";

/** One entry per artifact kind — the single place its "start a new one" UI
 *  comes from. `UtilitiesPanel` and `ArtifactsModal` both map over
 *  `ARTIFACT_KINDS` instead of hardcoding a button per kind, so adding a
 *  kind here is the only change either of them needs (same registry
 *  pattern as `src/data/utilities.ts`). The builder itself is a separate,
 *  smaller extension point: one line in `ArtifactView`'s `kind === "..."`
 *  dispatch, mirroring `UtilityView`. */
export type ArtifactKindDef = {
  id: ArtifactKind;
  /** Button text for starting a new one, e.g. "Новый HTTP-запрос". */
  newLabel: string;
  /** Utilities-panel card title, e.g. "Конструктор HTTP-запроса". */
  cardTitle: string;
  cardDescription: string;
  icon: LucideIcon;
};

export const ARTIFACT_KINDS: ArtifactKindDef[] = [
  {
    id: "httpRequest",
    newLabel: "Новый HTTP-запрос",
    cardTitle: "Конструктор HTTP-запроса",
    cardDescription: "Метод, параметры, тело и ответы — с готовым AsciiDoc для документации",
    icon: FileInput,
  },
];
