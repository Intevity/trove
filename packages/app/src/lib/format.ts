/** Grouped integer for KPI tiles. Uses the runtime locale, so 1234567
 *  renders as "1,234,567" in en-US, "1.234.567" in de-DE, etc. */
export function formatCount(n: number): string {
  return n.toLocaleString();
}

/** Absolute clock-time of the last signal, derived from the two
 *  `*MsAgo` scalars on a metrics snapshot. The snapshot's
 *  `scrapedMsAgo` anchors it to a moment slightly in the past;
 *  subtracting `lastSignalMsAgo` from that anchor gives the actual
 *  event time. Caller must guard `lastSignalMsAgo !== null`. */
export function formatLastSignalAt(scrapedMsAgo: number, lastSignalMsAgo: number): string {
  const t = new Date(Date.now() - scrapedMsAgo - lastSignalMsAgo);
  return t.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' });
}
