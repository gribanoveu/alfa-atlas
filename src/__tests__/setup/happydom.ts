// Registers happy-dom's globals (document, window, …) before any test runs,
// so `@testing-library/react` can render. Loaded via `bunfig.toml`'s
// `[test] preload` — Bun's own runner has no DOM of its own.
//
// The pure-function tests in this directory neither need nor notice it.
import { GlobalRegistrator } from "@happy-dom/global-registrator";

GlobalRegistrator.register();
