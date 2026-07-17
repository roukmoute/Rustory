import React from "react";
import ReactDOM from "react-dom/client";
import { RouterProvider } from "react-router-dom";

import "./shared/ui/tokens.css";
import { bootstrapDropSignals } from "./app/drop-bootstrap";
import { bootstrapOsOpenSignal } from "./app/os-open-bootstrap";
import { router } from "./app/router";

// Wire the OS-open and drop signals ONCE, outside the React lifecycle
// (StrictMode double-mounts effects, never this module scope).
bootstrapOsOpenSignal();
bootstrapDropSignals();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
);
