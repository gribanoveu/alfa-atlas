import { artifactCreateDraft, type ArtifactKind } from "./artifacts";

/** Opening artifact builder tabs: the tab-id encoding (mirroring
 *  `utilityTabId` in `src/data/utilities.ts` and `planTabId` in
 *  `useEditorTabs`) plus the "create one and open it" action every entry
 *  point outside the chat shares.
 *
 *  Opening happens through the `atlas-open-artifact` window event rather
 *  than a prop chain — the same cross-component escape hatch
 *  `atlas-open-plan` already uses, since the components that trigger it
 *  (the utilities panel, the artifacts dialog, a chat card) sit in three
 *  different subtrees from the editor. */

const TAB_ID_PREFIX = "artifact:";

export function artifactTabId(artifactId: string): string {
  return `${TAB_ID_PREFIX}${artifactId}`;
}

export function artifactIdFromTabId(tabId: string): string | null {
  if (!tabId.startsWith(TAB_ID_PREFIX)) return null;
  const id = tabId.slice(TAB_ID_PREFIX.length);
  return id.length > 0 ? id : null;
}

/** Dispatches the request to open `artifactId`'s builder tab. `App` listens
 *  for this. */
export function openArtifactTab(artifactId: string): void {
  window.dispatchEvent(new CustomEvent("atlas-open-artifact", { detail: { artifactId } }));
}

/** Creates an empty draft of `kind` and opens its builder. Used by the
 *  entry points that are not answering an assistant request — the chat
 *  card creates its own draft instead, seeded with the model's prefill.
 *  `title` is the seed shown before the user renames it; callers pass the
 *  matching `ArtifactKindDef.newLabel` from `src/data/artifactKinds.ts`. */
export async function createAndOpenArtifact(kind: ArtifactKind, title: string): Promise<void> {
  const record = await artifactCreateDraft({ kind, title });
  openArtifactTab(record.id);
}
