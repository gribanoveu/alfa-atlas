/// <reference types="vite/client" />

// PlantUML engine (TeaVM-compiled ES module). The shipped package has
// no type declarations; declare the public surface we use.
declare module "@plantuml/core" {
  export function render(
    lines: string[],
    targetId: string,
    options?: { dark?: boolean },
  ): void;
  export function renderToString(
    lines: string[],
    onSuccess: (svg: string) => void,
    onError: (err: string) => void,
  ): void;
}
