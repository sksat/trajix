import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

// Google Analytics: load only when VITE_GA_MEASUREMENT_ID is set (production build)
const gaId = import.meta.env.VITE_GA_MEASUREMENT_ID as string | undefined;
if (gaId) {
  const script = document.createElement("script");
  script.async = true;
  script.src = `https://www.googletagmanager.com/gtag/js?id=${gaId}`;
  document.head.appendChild(script);

  window.dataLayer = window.dataLayer || [];
  function gtag(...args: unknown[]) {
    window.dataLayer.push(args);
  }
  gtag("js", new Date());
  gtag("config", gaId);
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
