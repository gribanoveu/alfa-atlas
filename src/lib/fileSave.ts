import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { suggestedFileName, type BinaryContentInfo } from "./base64File";

export type SaveFileFilter = {
  name: string;
  extensions: string[];
};

export type SaveBytesOptions = {
  defaultPath?: string;
  filters?: SaveFileFilter[];
};

/** Returns true when saved, false when the user cancelled the dialog. */
export async function saveBytesViaDialog(
  bytes: Uint8Array,
  options: SaveBytesOptions,
): Promise<boolean> {
  const path = await save({
    defaultPath: options.defaultPath,
    filters: options.filters,
  });
  if (!path) {
    return false;
  }

  await writeFile(path, bytes);
  return true;
}

export async function saveDecodedBinaryFile(
  bytes: Uint8Array,
  content: BinaryContentInfo,
): Promise<boolean> {
  return saveBytesViaDialog(bytes, {
    defaultPath: suggestedFileName(content),
    filters: [{ name: content.label, extensions: [content.extension] }],
  });
}
