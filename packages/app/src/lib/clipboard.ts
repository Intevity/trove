/** Best-effort clipboard write. Returns `true` on success, `false` if
 *  the platform Clipboard API is unavailable or rejected. Mirrors the
 *  defensive pattern previously inlined in SyntheticSpanHints and
 *  HarnessSetupGuide — clipboard access can fail in non-secure
 *  contexts or under sandbox, and the callers all want a soft fallback
 *  rather than a thrown error. */
export async function copyToClipboard(value: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      /* fall through */
    }
  }
  return false;
}
