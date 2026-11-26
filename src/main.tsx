import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Osd from "./Osd";

const isOsd = window.location.hash === "#osd";
document.documentElement.dataset.view = isOsd ? "osd" : "flyout";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isOsd ? <Osd /> : <App />}</React.StrictMode>,
);
