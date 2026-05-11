import type { DetectedHarness, HarnessId } from '@trove/shared';

import { TroveIpcError } from '../../lib/ipc.js';
import { HarnessList } from '../HarnessList.js';

interface Props {
  harnesses: DetectedHarness[];
  loading: boolean;
  detectionError: TroveIpcError | null;
  busyIds: Set<HarnessId>;
  revertError: TroveIpcError | null;
  onEnable: (id: HarnessId) => void;
  onDisable: (id: HarnessId) => void;
  onRefresh: () => void;
}

export function HarnessesTab({
  harnesses,
  loading,
  detectionError,
  busyIds,
  revertError,
  onEnable,
  onDisable,
  onRefresh,
}: Props): JSX.Element {
  return (
    <div className="flex flex-col gap-4 px-4 py-3">
      {detectionError ? (
        <p
          className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
          data-testid="harness-list-error"
        >
          Detection failed: {detectionError.cause.kind}
        </p>
      ) : (
        <HarnessList
          harnesses={harnesses}
          loading={loading}
          onEnable={onEnable}
          onDisable={onDisable}
          busyIds={busyIds}
          onRefresh={onRefresh}
        />
      )}

      {revertError ? (
        <p
          className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
          data-testid="revert-error"
        >
          Revert failed: {revertError.cause.kind}
        </p>
      ) : null}
    </div>
  );
}
