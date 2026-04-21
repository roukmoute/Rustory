import React from "react";
import ReactDOM from "react-dom/client";

import "./shared/ui/tokens.css";
import { AppShell } from "./app/AppShell";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppShell />
  </React.StrictMode>,
);
