// ── Trajectory Render Error Boundary (debug aid) ─────────────────────────
//
// Catches render-time crashes inside the trajectory view so a white screen is
// replaced by a red panel showing the actual error + component stack.

import React from 'react';

interface Props {
  children: React.ReactNode;
}

interface State {
  error: Error | null;
  info: { componentStack: string } | null;
}

export class TrajectoryErrorBoundary extends React.Component<Props, State> {
  state: State = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack: string }) {
    // eslint-disable-next-line no-console
    console.error('[trajectory] render crash:', error, error?.stack ?? '', info.componentStack);
    this.setState({ info });
  }

  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <div className="p-4 m-3 rounded-lg border border-red-400 bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300 overflow-auto">
        <div className="text-xs font-semibold mb-2">Trajectory render crashed</div>
        <pre className="text-[11px] whitespace-pre-wrap break-all leading-relaxed">
          {String(this.state.error)}
        </pre>
        {this.state.error?.stack && (
          <pre className="text-[10px] whitespace-pre-wrap break-all leading-relaxed mt-2 opacity-90">
            {this.state.error.stack}
          </pre>
        )}
        {this.state.info?.componentStack && (
          <pre className="text-[10px] whitespace-pre-wrap break-all leading-relaxed mt-2 opacity-70">
            {this.state.info.componentStack}
          </pre>
        )}
      </div>
    );
  }
}