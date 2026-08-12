import { createContext, useContext, type ReactNode } from "react";

export type AscPreviewContextValue = {
  docsRoot: string | null;
  /** Docs-relative path of the file being previewed. */
  filePath: string | null;
};

const AscPreviewContext = createContext<AscPreviewContextValue>({
  docsRoot: null,
  filePath: null,
});

export function AscPreviewProvider({
  value,
  children,
}: {
  value: AscPreviewContextValue;
  children: ReactNode;
}) {
  return (
    <AscPreviewContext.Provider value={value}>{children}</AscPreviewContext.Provider>
  );
}

export function useAscPreview(): AscPreviewContextValue {
  return useContext(AscPreviewContext);
}
