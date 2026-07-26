import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./monacoSetup";
import "./styles/tokens.css";
import "./styles/app.css";
import "./styles/scrollbars.css";

// Suppress WebView system menu (Reload / Inspect Element) on empty areas.
// App-specific menus still call preventDefault on their own targets.
document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
