import React from "react";
import ReactDOM from "react-dom/client";
import { flushSync } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { isTauri, writeFrontendLog } from "./lib/bridge";
import "./styles.css";

function describeUnknown(value: unknown): string {
  if (value instanceof Error) return `${value.name}: ${value.message}\n${value.stack ?? ""}`;
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

window.addEventListener("error", (event) => {
  const location = event.filename
    ? ` at ${event.filename}:${event.lineno}:${event.colno}`
    : "";
  void writeFrontendLog(
    "error",
    `${event.message}${location}\n${event.error instanceof Error ? event.error.stack ?? "" : ""}`,
    "window.error"
  ).catch(() => undefined);
});

window.addEventListener("unhandledrejection", (event) => {
  void writeFrontendLog(
    "error",
    describeUnknown(event.reason),
    "window.unhandledrejection"
  ).catch(() => undefined);
});

interface ErrorBoundaryState {
  failed: boolean;
}

class FrontendErrorBoundary extends React.Component<React.PropsWithChildren, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    void writeFrontendLog(
      "error",
      `${describeUnknown(error)}\ncomponent stack:\n${info.componentStack ?? ""}`,
      "react.error-boundary"
    ).catch(() => undefined);
  }

  render() {
    if (this.state.failed) {
      return (
        <div className="frontend-crash">
          <b>DCreel 界面发生错误</b>
          <span>错误详情已经写入日志。</span>
          <button type="button" onClick={() => window.location.reload()}>
            重新加载
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

const root = ReactDOM.createRoot(document.getElementById("root")!);
const render = () => root.render(
  <React.StrictMode>
    <FrontendErrorBoundary>
      <App />
    </FrontendErrorBoundary>
  </React.StrictMode>
);

// Auxiliary windows start hidden. Mount their listeners without waiting for a
// browser paint/idle task, since native visibility now waits for a render ACK.
if (isTauri() && ["desktop-notification", "transfer-progress"].includes(getCurrentWindow().label)) {
  flushSync(render);
} else {
  render();
}
