import React from "react";
import ReactDOM from "react-dom/client";
import { RouterProvider } from "react-router-dom";

import "./shared/ui/tokens.css";
import { bootstrapOsOpenSignal } from "./app/os-open-bootstrap";
import { router } from "./app/router";

// Wire the OS-open signal ONCE, outside the React lifecycle (StrictMode
// double-mounts effects, never this module scope).
bootstrapOsOpenSignal();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
);
