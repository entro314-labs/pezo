import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/** Last-resort catch so a rendering bug degrades to a message, not a white window. */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
          <p className="text-lg font-semibold text-t1">Something broke in the interface</p>
          <p className="max-w-lg text-sm text-t2 select-text">{String(this.state.error)}</p>
          <button
            className="btn-secondary"
            onClick={() => this.setState({ error: null })}
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
