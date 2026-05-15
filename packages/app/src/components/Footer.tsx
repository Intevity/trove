import { open } from '@tauri-apps/plugin-shell';
import { Bug } from 'lucide-react';

import intevityLogo from '../assets/intevityLogoIcon.png';
import { openBugReport } from '../lib/bugReport.js';

const INTEVITY_URL =
  'https://www.intevity.com/?utm_source=trove&utm_medium=app&utm_campaign=built-by-footer';

const displayVersion = __APP_VERSION__.startsWith('v')
  ? __APP_VERSION__
  : __APP_VERSION__ === 'dev'
    ? 'dev'
    : `v${__APP_VERSION__}`;

export function Footer(): JSX.Element {
  const handleOpenIntevity = (): void => {
    void open(INTEVITY_URL);
  };

  const handleReportBug = (): void => {
    void openBugReport({ source: 'manual' });
  };

  return (
    <footer
      data-testid="app-footer"
      className="flex flex-shrink-0 items-center justify-between border-t border-hairline px-4 py-1.5 text-[10px] text-ios-gray dark:border-hairline-dark"
    >
      <div className="flex items-center gap-2">
        <span className="font-mono tabular-nums" title={`Trove ${displayVersion}`}>
          {displayVersion}
        </span>
        <span aria-hidden="true" className="h-3 w-px bg-black/15 dark:bg-white/15" />
        <button
          type="button"
          onClick={handleReportBug}
          data-testid="footer-report-bug"
          aria-label="Report a bug"
          title="Report a bug"
          className="flex items-center gap-1 rounded-md px-1 py-0.5 transition-colors hover:text-brand focus:outline-none focus-visible:ring-2 focus-visible:ring-brand dark:hover:text-brand"
        >
          <Bug size={11} strokeWidth={2.2} />
          <span>Report</span>
        </button>
      </div>
      <button
        type="button"
        onClick={handleOpenIntevity}
        data-testid="footer-built-by"
        aria-label="Built by Intevity; open intevity.com"
        className="flex items-center gap-1.5 rounded-md px-1 py-0.5 transition-colors hover:text-brand focus:outline-none focus-visible:ring-2 focus-visible:ring-brand dark:hover:text-brand"
      >
        <span>Built by</span>
        <img src={intevityLogo} alt="Intevity" className="h-3.5 w-auto" />
      </button>
    </footer>
  );
}
