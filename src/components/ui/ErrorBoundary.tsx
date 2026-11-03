import { AlertTriangle } from "lucide-react";
import React from "react";
import { Button } from "./index";

interface State {
  error: Error | null;
}

/** Keeps a failure in one view (or a failed lazy chunk) from taking down the whole window. */
export class ErrorBoundary extends React.Component<{ children: React.ReactNode; label?: string; onReset?: () => void }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error(`[${this.props.label ?? "view"}]`, error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex h-full min-h-48 flex-col items-center justify-center gap-3 p-6 text-center">
        <div className="flex size-10 items-center justify-center rounded-lg bg-err-soft text-err">
          <AlertTriangle className="size-5" />
        </div>
        <div className="text-[14px] font-medium">Something went wrong in {this.props.label ?? "this view"}</div>
        <div className="selectable max-w-lg text-[12.5px] text-muted">{this.state.error.message}</div>
        <Button
          onClick={() => {
            this.setState({ error: null });
            this.props.onReset?.();
          }}
        >
          Try again
        </Button>
      </div>
    );
  }
}
