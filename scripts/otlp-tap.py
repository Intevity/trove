#!/usr/bin/env python3
"""
otlp-tap.py — watch the Trove collector for incoming OTLP metric points.

Polls the collector's Prometheus self-metrics endpoint every 2 s, diffs
the per-pipeline counters, and prints a summary whenever new metric points
arrive. Designed for smoke-testing watcher adapters (Droid, Gemini, etc.)
without restarting the collector or changing any endpoints.

Usage:
    python3 scripts/otlp-tap.py               # watch all harnesses
    python3 scripts/otlp-tap.py --harness droid  # filter to one harness

The collector must be running (Trove app open) before you start this.
Quit with Ctrl-C.
"""
import argparse
import re
import sys
import time
import urllib.error
import urllib.request
from collections import defaultdict

PROM_URL = "http://127.0.0.1:18888/metrics"
POLL_INTERVAL_S = 2


def fetch_metrics(url: str) -> str:
    try:
        with urllib.request.urlopen(url, timeout=3) as r:
            return r.read().decode()
    except urllib.error.URLError as e:
        return f"# ERROR: {e}"


def parse_counters(text: str) -> dict[str, float]:
    """Return {metric_label_string: value} for all non-comment, non-# lines."""
    out: dict[str, float] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.rsplit(None, 1)
        if len(parts) != 2:
            continue
        key, val = parts
        try:
            out[key] = float(val)
        except ValueError:
            pass
    return out


PIPELINE_RE = re.compile(
    r'otelcol_(?P<kind>receiver_accepted|processor_refused|'
    r'exporter_sent|exporter_enqueue_failed|exporter_send_failed)'
    r'_(?P<signal>metric_points|spans|log_records)'
    r'(?:_total)?\{'
    r'(?P<labels>[^}]*)\}'
)

# Per-harness diag counters: otelcol_processor_outgoing_items_total with
# otel_signal="metrics" and processor="filter/diag-<harness>". These are
# the only counters that carry the harness name when Trove's pipelines use
# a shared OTLP receiver/exporter (all harnesses share receiver="otlp").
DIAG_OUTGOING_RE = re.compile(
    r'otelcol_processor_outgoing_items_total\{'
    r'(?P<labels>[^}]*)\}'
)


def extract_pipeline_rows(counters: dict[str, float]) -> list[dict]:
    rows = []
    for key, val in counters.items():
        m = PIPELINE_RE.match(key)
        if m:
            labels = dict(
                part.split("=", 1)
                for part in m.group("labels").split(",")
                if "=" in part
            )
            labels = {k: v.strip('"') for k, v in labels.items()}
            rows.append(
                {
                    "key": key,
                    "kind": m.group("kind"),
                    "signal": m.group("signal"),
                    "labels": labels,
                    "value": val,
                }
            )
            continue
        # Also capture per-harness diag pipeline outgoing counters (metrics
        # only) — these carry the harness name via processor="filter/diag-<h>"
        # and are the only harness-filterable counters when all harnesses
        # share a single OTLP receiver.
        md = DIAG_OUTGOING_RE.match(key)
        if md:
            labels = dict(
                part.split("=", 1)
                for part in md.group("labels").split(",")
                if "=" in part
            )
            labels = {k: v.strip('"') for k, v in labels.items()}
            if labels.get("otel_signal") != "metrics":
                continue
            proc = labels.get("processor", "")
            if not proc.startswith("filter/diag-"):
                continue
            rows.append(
                {
                    "key": key,
                    "kind": "diag_outgoing",
                    "signal": "metric_points",
                    "labels": labels,
                    "value": val,
                }
            )
    return rows


def harness_filter(row: dict, harness: str | None) -> bool:
    if harness is None:
        return True
    name = row["labels"].get("processor", row["labels"].get("exporter", ""))
    receiver = row["labels"].get("receiver", "")
    return harness.lower() in name.lower() or harness.lower() in receiver.lower()


def fmt_delta(rows_before: dict, rows_after: dict, harness: str | None) -> list[str]:
    lines = []
    for key, after_val in rows_after.items():
        before_val = rows_before.get(key, 0.0)
        delta = after_val - before_val
        if delta <= 0:
            continue
        # Parse labels for a human-friendly summary
        m = PIPELINE_RE.match(key)
        if m:
            labels = dict(
                part.split("=", 1)
                for part in m.group("labels").split(",")
                if "=" in part
            )
            labels = {k: v.strip('"') for k, v in labels.items()}
            component = (
                labels.get("processor")
                or labels.get("exporter")
                or labels.get("receiver")
                or "?"
            )
            if harness and harness.lower() not in component.lower():
                continue
            kind = m.group("kind")
            lines.append(
                f"  +{delta:>8.0f}  {m.group('signal'):<14}  "
                f"{kind:<30}  {component}"
            )
        else:
            # Try the diag-outgoing pattern before falling back to raw key.
            md = DIAG_OUTGOING_RE.match(key)
            if md:
                dlabels = dict(
                    part.split("=", 1)
                    for part in md.group("labels").split(",")
                    if "=" in part
                )
                dlabels = {k: v.strip('"') for k, v in dlabels.items()}
                if dlabels.get("otel_signal") != "metrics":
                    continue
                proc = dlabels.get("processor", "")
                if not proc.startswith("filter/diag-"):
                    continue
                if harness and harness.lower() not in proc.lower():
                    continue
                lines.append(
                    f"  +{delta:>8.0f}  {'metric_points':<14}  "
                    f"{'diag_outgoing':<30}  {proc}"
                )
            else:
                # Suppress noisy internal counters that duplicate the
                # diag_outgoing line or carry no harness-filterable signal:
                #   - processor_incoming_items_total  → always == outgoing
                #     when no records are dropped; outgoing is more useful
                #   - processor_outgoing_items_total for non-diag processors
                #     (already captured above via DIAG_OUTGOING_RE for diag-*)
                if "processor_incoming_items_total" in key:
                    continue
                if "processor_outgoing_items_total" in key:
                    continue
                if harness and harness.lower() not in key.lower():
                    continue
                lines.append(f"  +{delta:>8.0f}  {key}")
    return sorted(lines)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--harness", metavar="NAME", default=None,
                    help="Filter output to a specific harness/pipeline name substring (e.g. 'droid')")
    ap.add_argument("--url", default=PROM_URL, metavar="URL",
                    help=f"Prometheus metrics URL (default: {PROM_URL})")
    ap.add_argument("--interval", type=float, default=POLL_INTERVAL_S, metavar="SECS",
                    help=f"Poll interval in seconds (default: {POLL_INTERVAL_S})")
    args = ap.parse_args()

    print(f"Watching {args.url} every {args.interval}s — Ctrl-C to quit")
    if args.harness:
        print(f"Filter: {args.harness!r}")
    print()

    # Warm-up: get baseline before the user does anything.
    prev_raw = fetch_metrics(args.url)
    if prev_raw.startswith("# ERROR"):
        print(prev_raw, file=sys.stderr)
        print("Is Trove running?", file=sys.stderr)
        sys.exit(1)
    prev_counters = parse_counters(prev_raw)
    print("Baseline captured. Run Droid (or trigger the watcher) now.\n")

    try:
        while True:
            time.sleep(args.interval)
            raw = fetch_metrics(args.url)
            if raw.startswith("# ERROR"):
                print(f"\n{raw}")
                prev_counters = {}
                continue
            counters = parse_counters(raw)
            deltas = fmt_delta(prev_counters, counters, args.harness)
            if deltas:
                ts = time.strftime("%H:%M:%S")
                print(f"[{ts}] new metric points:")
                for d in deltas:
                    print(d)
                print()
            prev_counters = counters
    except KeyboardInterrupt:
        print("\nDone.")


if __name__ == "__main__":
    main()
