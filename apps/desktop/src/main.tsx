import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import App from "./App";

async function boot() {
  // Dev-only visual preview bridge; the condition is statically false in production builds
  // so the module (and its data) is tree-shaken out of dist.
  if (import.meta.env.DEV && !(window as any).__TAURI_INTERNALS__) {
    await import("./lib/devPreview");
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}
void boot();
