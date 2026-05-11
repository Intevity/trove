import { LogsPanel } from '../LogsPanel.js';

export function LogsTab(): JSX.Element {
  return (
    <div className="flex flex-col h-full px-4 py-3">
      <LogsPanel />
    </div>
  );
}
