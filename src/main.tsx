import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./App.css";

// ─── Error Boundary ─────────────────────────────────────────

interface EBState { hasError: boolean; error?: Error }

class ErrorBoundary extends React.Component<{ children: React.ReactNode }, EBState> {
  state: EBState = { hasError: false };

  static getDerivedStateFromError(error: Error): EBState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("Pulse ErrorBoundary:", error, info);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="popup" style={{ alignItems: "center", justifyContent: "center", textAlign: "center", gap: 14 }}>
          <div style={{ fontSize: 32, opacity: 0.4 }}>⚠</div>
          <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>界面渲染异常</div>
          <div style={{ fontSize: 12, color: "var(--text-secondary)", maxWidth: 240 }}>
            {this.state.error?.message ?? "未知错误"}
          </div>
          <button
            className="btn btn-primary btn-xs"
            onClick={() => this.setState({ hasError: false })}
          >
            重试
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

// ─── Mount ──────────────────────────────────────────────────

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
