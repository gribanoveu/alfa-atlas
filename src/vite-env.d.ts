/// <reference types="vite/client" />

// Vendored PlantUML engine (TeaVM-compiled ES module). The shipped file has
// no type declarations; declare the public surface we use.
declare module "*/vendor/plantuml/plantuml.js" {
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
