import React from "react";
import ReactDOM from "react-dom/client";

import { AppShell } from "./app/AppShell";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppShell />
  </React.StrictMode>,
);
