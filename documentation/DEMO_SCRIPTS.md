# Trove Demo Recording Scripts

Master script for an AI agent (or a human operator) driving a screen-recording
tool against the **running Trove desktop app** to record each marketing demo
clip deterministically. One numbered section per clip; follow the steps
verbatim, no judgement calls.

> Copyright (c) 2026 Intevity. Trove is MIT-licensed and open source:
> <https://github.com/Intevity/trove>

---

## 0. Targets, output, and the post-record handoff

- **Target app:** Trove desktop (Tauri 2 tray app), **dark theme**, current
  version **0.5.0**.
- **Recording resolution:** capture the Trove window at **1920x1080** (16:9).
  Record only the app window, not the whole desktop. No menu bar, no other
  windows, no notifications.
- **Frame rate:** 60 fps preferred, 30 fps acceptable. Motion in Trove (flow
  lines, halos, the Orbital Hub) reads better at 60.
- **Each clip maps to a file:**
  - Video -> `packages/site/public/videos/<slug>.mp4` (H.264, yuv420p, no audio
    track required — captions are burned in later or rendered by the site).
  - Poster frame -> `packages/site/public/videos/<slug>.jpg`, extracted from the
    finished clip with ffmpeg. Pick a frame ~1.2 s in, after the intro settles:

    ```sh
    ffmpeg -ss 1.2 -i <slug>.mp4 -vframes 1 -q:v 2 <slug>.jpg
    ```

- **After a real clip lands, flip the data flags.** Until a clip exists, the
  site shows the branded SVG placeholder at
  `packages/site/public/videos/<slug>.svg`. Once `<slug>.mp4` + `<slug>.jpg`
  are committed, edit the matching entry in
  `packages/site/src/data/features.ts` **or** `packages/site/src/data/pillars.ts`
  (see each section's "Maps to" line for which file):
  1. set `hasVideo: true`
  2. change `poster: "/videos/<slug>.svg"` -> `poster: "/videos/<slug>.jpg"`
     Leave the other entries on their SVG placeholders until their clips land.

- **Encode settings (reference):**

  ```sh
  ffmpeg -i raw-capture.mov -vf "scale=1920:1080:flags=lanczos" \
    -c:v libx264 -profile:v high -pix_fmt yuv420p -crf 20 \
    -movflags +faststart -an packages/site/public/videos/<slug>.mp4
  ```

---

## 1. General setup and preconditions (do this once, before any clip)

### 1.1 Launch and theme

1. Launch Trove. If it lives only in the tray, click the tray icon and choose
   **Open Trove** (or **Show Dashboard**) to bring up the main window.
2. Open **Settings** (top-right tab). Confirm appearance is **Dark**. If a Theme
   control exists, set it to **Dark**; otherwise set the OS to Dark Mode so the
   WKWebView renders dark. The canvas must read `#1c1c1e`, accents Trove teal
   `#2dbfb8`.
3. Resize the window to **1920x1080** (or as close as the OS allows, then crop
   the capture region to exactly 1920x1080). Center it on a solid neutral
   desktop background.

### 1.2 Stage demo data so the dashboard looks populated

The Overview screenshots in `documentation/screenshots/` show the intended
"good" state. Reproduce it:

1. **Enable 3 harnesses** on the Harnesses tab so the Data flow chart renders
   **individual nodes** (<= 3 tools = individual nodes; 4+ collapse into the
   animated **Orbital Hub** cluster). Recommended trio for individual-node
   clips: **Claude Code**, **OpenAI Codex CLI**, **Cursor IDE** (all Native
   OTel, so they go green fast).
2. **Enable 2-3 platforms** on the Platforms tab — e.g. **SigNoz Cloud**
   (recommended), **Grafana**, and one more — all pointed at **localhost**
   collector ports so nothing leaves the machine.
3. **Generate live telemetry** so halos light and the Collector counters move.
   Either run real short sessions in the enabled harnesses, or use the
   built-in **Test Pipeline** button (Overview -> Collector card) to inject a
   synthetic signal burst. The Overview "Recent signal" should read
   "last signal Ns ago" with N small (single digits) during recording.
4. For the **Orbital Hub** clip only: enable **4 or more** harnesses so the flow
   chart collapses sources into the animated cluster.

### 1.3 Redaction (mandatory before recording)

The real app surfaces machine-specific strings. Before recording, ensure none
of the following appear on screen, or blur/crop them in post:

- **Real OTLP tokens / ingestion keys / Basic-auth values / API keys** — never
  show a populated credential field. Use placeholder creds or a redacted value.
- **Real emails** — Settings shows identity tagging `user.name` / `user.email`.
  Set these to demo values (e.g. `demo@trove.dev`, `Demo User`) before
  recording the Settings/identity beat.
- **Real org / tenant / dataset names** in platform endpoints (Datadog site,
  OpenObserve `org`, Chronosphere `<tenant>`, Honeycomb dataset). Use generic
  demo values.
- **Home-directory usernames in config paths** — the Harnesses rows print paths
  like `/Users/<you>/.claude/settings.json`. Either record under a demo user
  account, or crop/blur the path subtitle. (Endpoints shown as `localhost:PORT`
  are fine to show.)

### 1.4 Bookend convention (used by every section)

- **Start frame:** the clip's first interactive frame — the named tab already
  selected, window settled, cursor parked off the active control. A branded
  teal intro card is wrapped on later in post, so the agent just needs a clean,
  static first frame.
- **End frame:** a clean, static last frame holding the clip's "money shot" for
  ~1 s with no cursor movement, so a branded teal outro card can cross-fade in.
- Keep the cursor still for the first and last ~0.8 s of every recording.

---

## 2. Clip scripts

> Durations are wall-clock targets for the raw action (before intro/outro
> bookends). "Maps to" names the carousel feature or pillar and the data file
> whose flags you flip once the clip lands.

### Section 1 — `detect`

- **Slug:** `detect`
- **Title:** Auto-detect harnesses
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 12-15 s
- **Tab/screen:** Harnesses
- **Preconditions/data state:** App on the Harnesses tab. Several harnesses
  detected, a mix of states (some Telemetry on / off / unknown, badges
  Auto-detected / Partial Coverage / Best Effort visible). Redact config paths.
- **Steps:**
  1. Start on the Harnesses tab, list fully rendered, cursor parked top-right.
  2. Wait 1.0 s on the static list (let the viewer read "Detected harnesses").
  3. Click **Refresh** (top-right of the list).
  4. Wait for the re-scan to complete and rows to re-render (~1.5 s).
  5. Slowly scroll the list down ~600 px over ~3 s to reveal the full set of
     detected tools, pausing ~0.5 s on the Best Effort rows (Aider, Copilot
     CLI) so their badges are legible.
  6. Scroll back to top over ~1.5 s.
  7. Hover (do not click) the telemetry pill on the first Native OTel row to
     surface its tooltip; hold 1.0 s. Park cursor. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "Trove sweeps every install path on launch."
  - 4.0 s: "17 AI coding tools, auto-detected."
  - 9.0 s: "Native OTel, partial, and best-effort — labeled."
- **Start frame:** Harnesses tab, top of the detected list.
- **End frame:** Top of list with one telemetry-pill tooltip visible.
- **Success criteria:** Multiple harnesses with names, detection-source
  subtitles, telemetry pills, and at least one each of Auto-detected / Partial
  Coverage / Best Effort badges are clearly visible. No real config username.

### Section 2 — `enable`

- **Slug:** `enable`
- **Title:** One-click enable / disable
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 10-14 s
- **Tab/screen:** Harnesses
- **Preconditions/data state:** Pick one Native OTel harness currently
  **disabled** (e.g. Qwen Code or OpenCode, "Telemetry off", showing an
  **Enable** button). Collector running.
- **Steps:**
  1. Start on Harnesses tab. Cursor near the chosen disabled row.
  2. Wait 0.8 s. Hover the **Enable** button on that row (hold 0.5 s).
  3. Click **Enable**.
  4. Hold while the row transitions: the pill flips to **Telemetry on** and the
     status dot turns green (~1.5 s). Do not move the cursor during the flip.
  5. Wait 1.0 s on the green state.
  6. Hover the now-visible **Disable** button (hold 0.5 s), click **Disable**.
  7. Hold while it reverts to "Telemetry off" (~1.5 s). Re-enable it once more
     (click **Enable**) so the clip ends in the green, enabled state.
  8. Park cursor. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "One toggle writes a managed region into the tool's own config."
  - 5.0 s: "Green the moment OTLP starts flowing."
  - 9.0 s: "Disable reverts it, byte-for-byte."
- **Start frame:** Harnesses row in disabled state.
- **End frame:** Same row, enabled and green.
- **Success criteria:** The viewer sees a row visibly transition
  disabled -> enabled (pill + dot), then enabled -> disabled, then back to
  enabled. Smooth, no error toast.

### Section 3 — `fan-out`

- **Slug:** `fan-out`
- **Title:** Multi-backend fan-out
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Platforms, then Overview
- **Preconditions/data state:** 3 platforms configured (e.g. SigNoz Cloud,
  Grafana, OpenObserve), all **Enabled**, all on localhost endpoints. Telemetry
  flowing.
- **Steps:**
  1. Start on the Platforms tab, list rendered, multiple platforms **Enabled**.
  2. Wait 1.0 s. Slowly scroll ~300 px to show 3+ enabled platforms with green
     "Enabled" pills; pause 0.5 s.
  3. Click **Disable** on one platform (e.g. OpenObserve); hold 1.0 s as the
     pill greys to "disabled" (credentials retained).
  4. Click **Enable** on that same platform; hold 1.0 s as it returns to green.
  5. Click the **Overview** tab.
  6. On the Data flow chart, hold 3 s while flow lines animate from the
     Collector out to **all enabled** destination nodes simultaneously.
  7. Park cursor on the Collector node. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "Configure once."
  - 4.0 s: "Every signal broadcasts to every enabled backend."
  - 9.0 s: "Disable pauses forwarding without losing credentials."
- **Start frame:** Platforms tab with multiple Enabled pills.
- **End frame:** Overview flow chart, lines fanning out to multiple backends.
- **Success criteria:** At least 3 destination platforms enabled; on Overview,
  flow lines reach 2+ backend nodes at once. No credential values shown.

### Section 4 — `metrics`

- **Slug:** `metrics`
- **Title:** Unified Tier A metrics
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Mappings
- **Preconditions/data state:** Mappings tab populated with at least one
  harness's synthesis rules feeding the Tier A schema. Live preview/diff
  available.
- **Steps:**
  1. Start on the Mappings tab, a harness's rules visible.
  2. Wait 1.0 s. Slowly scroll to reveal the five Tier A targets:
     `trove.harness.events`, `trove.harness.tokens`, `trove.harness.cost.usd`,
     `trove.harness.turn.duration`, `trove.harness.errors`. Pause 0.5 s on each
     group as it scrolls into view.
  3. Hover one synthesis rule's target metric chip to highlight the mapping
     (hold 1.0 s).
  4. Scroll back so the full Tier A list is framed. Park cursor.
  5. Hold end frame 1.0 s on the five Tier A metric names.
- **Captions / narration beats:**
  - 0.0 s: "Every harness speaks a different dialect."
  - 4.0 s: "Trove normalizes them onto one Tier A schema."
  - 9.0 s: "events, tokens, cost, turn duration, errors."
- **Start frame:** Mappings tab, rules in view.
- **End frame:** The five Tier A metric names framed and legible.
- **Success criteria:** All five Tier A metric names readable; at least one
  synthesis rule -> Tier A mapping visibly highlighted.

### Section 5 — `health`

- **Slug:** `health`
- **Title:** Live health monitoring
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 10-14 s
- **Tab/screen:** Overview (Diagnostics panel)
- **Preconditions/data state:** Sidecar running, 3 harnesses enabled, recent
  signal, backend exporting. Telemetry actively flowing so "Recent signal"
  stays low.
- **Steps:**
  1. Start on Overview, Diagnostics panel at top in view.
  2. Wait 1.0 s. Hover each Diagnostics row top-to-bottom, ~0.8 s each:
     **Sidecar** (running), **Harnesses** (N enabled), **Recent signal**
     (last signal Ns ago), **Backend** (exporting / records sent).
  3. Click **Run backend check** (top-right of Diagnostics).
  4. Hold 2.0 s as the check runs and the rows confirm healthy/green.
  5. Park cursor. Hold end frame 1.0 s on all-green Diagnostics.
- **Captions / narration beats:**
  - 0.0 s: "One glance: sidecar, harnesses, signal, backend."
  - 5.0 s: "Run a backend check on demand."
  - 9.0 s: "Green means data is flowing end-to-end."
- **Start frame:** Overview Diagnostics panel.
- **End frame:** Diagnostics all green after the backend check.
- **Success criteria:** All four Diagnostics rows show healthy state; the
  backend check completes without error. "Recent signal" is a small number.

### Section 6 — `mappings`

- **Slug:** `mappings`
- **Title:** Visual mappings editor
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 14-18 s
- **Tab/screen:** Mappings
- **Preconditions/data state:** A harness selected with editable synthesis
  rules and a live diff preview pane.
- **Steps:**
  1. Start on Mappings tab. Select a Native OTel harness (e.g. Claude Code) so
     synthesis rules show.
  2. Wait 1.0 s. Click into one synthesis rule to open/expand its editor.
  3. Change one mapping (e.g. point a raw counter at a different Tier A metric,
     or add an attribute filter). Type slowly so the change is visible.
  4. Hold 2.0 s while the **live diff preview** updates to show the before/after.
  5. Click **Apply** (or the equivalent save/apply control). Hold 1.5 s while a
     success indicator confirms changes take effect on next collector reload.
  6. Revert or leave the change as-is, then park cursor. Hold end frame 1.0 s
     on the diff preview.
- **Captions / narration beats:**
  - 0.0 s: "Map any raw harness signal onto Tier A — visually."
  - 6.0 s: "Live diff shows exactly what changes."
  - 11.0 s: "Apply lives — the collector reloads, no restart."
- **Start frame:** Mappings editor for a selected harness.
- **End frame:** Live diff preview reflecting an applied change.
- **Success criteria:** An edit visibly drives a live diff; Apply succeeds and
  confirms. No raw credentials shown.

### Section 7 — `overview`

- **Slug:** `overview`
- **Title:** Overview: one pane of glass
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 14-18 s
- **Tab/screen:** Overview
- **Preconditions/data state:** The full populated Overview: Diagnostics
  (sidecar running, 3 harnesses, recent signal, backend exporting), Data flow
  chart with 3 harness nodes -> Collector -> 2 backends, Collector counters
  (Received / Sent / Last signal) non-zero and moving.
- **Steps:**
  1. Start on Overview, top of page (Diagnostics in view), cursor parked.
  2. Wait 1.0 s. Slowly scroll the full page top-to-bottom over ~6 s:
     Diagnostics -> Data flow -> Collector counters. Pause ~1 s on the Data
     flow chart mid-scroll while lines animate.
  3. Trigger **Test Pipeline** (Collector card) once so the counters tick up
     and "Last signal" resets to a fresh value; hold 2.0 s.
  4. Scroll back to top over ~2 s. Park cursor.
  5. Hold end frame 1.0 s with the Data flow chart visible and active.
- **Captions / narration beats:**
  - 0.0 s: "Everything in one pane."
  - 5.0 s: "Health, live data flow, throughput."
  - 11.0 s: "One app for every tool on your machine."
- **Start frame:** Overview top (Diagnostics).
- **End frame:** Data flow chart with live lines + non-zero counters.
- **Success criteria:** Diagnostics, Data flow, and Collector counters all
  visible across the clip; counters change after Test Pipeline.

### Section 8 — `flow-chart`

- **Slug:** `flow-chart`
- **Title:** Live data-flow chart
- **Maps to:** features carousel — `features.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Overview (Data flow section)
- **Preconditions/data state:** **4+ harnesses enabled** so the chart collapses
  sources into the animated **Orbital Hub** cluster; telemetry flowing so
  activity halos pulse. 2+ backends enabled.
- **Steps:**
  1. Start on Overview, scrolled so the **Data flow** section fills the frame.
  2. Wait 1.0 s. Hold 3 s on the Orbital Hub cluster animating, with the legend
     (Spans / Metrics / Logs) visible.
  3. Trigger **Test Pipeline** so source halos light and flow lines pulse from
     the hub through the Collector to the backend nodes; hold 3 s.
  4. Hover the Collector node to surface its status (running); hold 1.0 s.
  5. Park cursor. Hold end frame 1.0 s with lines mid-flow.
- **Captions / narration beats:**
  - 0.0 s: "Watch telemetry move in real time."
  - 5.0 s: "Four or more tools collapse into the Orbital Hub."
  - 10.0 s: "Spans, metrics, and logs, color-coded."
- **Start frame:** Data flow section, Orbital Hub at rest.
- **End frame:** Flow lines mid-animation from hub to backends.
- **Success criteria:** The Orbital Hub cluster is visible (not individual
  nodes); halos/lines animate; Spans/Metrics/Logs legend readable.

### Section 9 — `cost-normalization`

- **Slug:** `cost-normalization`
- **Title:** Cross-vendor cost normalization
- **Maps to:** pillars — `pillars.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Mappings (Tier A cost focus); optionally Overview for
  throughput context.
- **Preconditions/data state:** Multiple harnesses enabled and emitting, so
  `trove.harness.cost.usd` and `trove.harness.tokens` are populated across
  vendors. Mappings show cost mapped from each harness's native counters.
- **Steps:**
  1. Start on Mappings. Wait 1.0 s.
  2. Scroll/select to show **two different harnesses** (e.g. Claude Code and
     Codex CLI) both mapping native counters onto the same
     `trove.harness.cost.usd` and `trove.harness.tokens` Tier A metrics.
  3. Hover the `trove.harness.cost.usd` chip on each harness in turn (hold
     1.0 s each) to underscore they land on the same metric.
  4. Hold 2.0 s framing both harnesses' cost mappings side by side.
  5. Park cursor. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "Every vendor counts tokens and cost differently."
  - 5.0 s: "Trove maps them all to one cost metric."
  - 10.0 s: "Cost per turn, comparable across tools."
- **Start frame:** Mappings showing a harness's cost mapping.
- **End frame:** Two harnesses' cost mappings sharing the Tier A cost metric.
- **Success criteria:** `trove.harness.cost.usd` (and ideally
  `trove.harness.tokens`) shown as the shared target for 2+ harnesses.

### Section 10 — `dead-seats`

- **Slug:** `dead-seats`
- **Title:** Find dead seats
- **Maps to:** pillars — `pillars.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Harnesses (zero-activity contrast); narration carries the
  procurement story.
- **Preconditions/data state:** A deliberate mix: some harnesses **enabled and
  actively emitting** (green, recent signal) and at least one harness
  **enabled but with no recent activity** (telemetry on, but no signal — the
  "dead seat" stand-in). Redact usernames in paths.
- **Steps:**
  1. Start on Harnesses tab. Wait 1.0 s.
  2. Hover an **active** harness row (green, Telemetry on) — hold 1.0 s.
  3. Hover the **enabled-but-silent** harness row (Telemetry on but no recent
     signal / zero activity) — hold 1.5 s to contrast.
  4. (If a per-harness activity/last-seen indicator exists, frame it.)
     Otherwise cut to Overview and show "Recent signal" attribution while
     narration explains the zero-turn detection.
  5. Park cursor on the silent row. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "You pay for seats across every tool."
  - 5.0 s: "Trove keys every signal by harness and user."
  - 10.0 s: "Zero turns in 30 days? That's a seat to reclaim."
- **Start frame:** Harnesses list, mixed activity.
- **End frame:** The silent / zero-activity harness row in focus.
- **Success criteria:** A clear contrast between an active harness and an
  enabled-but-silent one is visible. No real org or user data shown.

### Section 11 — `localhost-only`

- **Slug:** `localhost-only`
- **Title:** Localhost-only, never phones home
- **Maps to:** pillars — `pillars.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Platforms (endpoints) and Logs (collector tail)
- **Preconditions/data state:** Platforms configured with **localhost** endpoint
  rows visible (e.g. `localhost:14317`, `http://localhost:14318`). Logs tab
  showing the live collector tail binding 127.0.0.1.
- **Steps:**
  1. Start on Platforms. Wait 1.0 s. Hover 2-3 platform rows so their
     **`localhost:PORT`** endpoints are clearly legible (hold 1.0 s each).
  2. Click the **Logs** tab.
  3. Hold 3 s on the live collector tail; if a line shows the collector binding
     `127.0.0.1`, frame it. Let several log lines stream.
  4. Park cursor. Hold end frame 1.0 s on the streaming localhost logs.
- **Captions / narration beats:**
  - 0.0 s: "The collector binds 127.0.0.1 — localhost only."
  - 5.0 s: "It forwards only to the backend you chose."
  - 10.0 s: "Never to Trove. Never to Intevity."
- **Start frame:** Platforms tab, localhost endpoints visible.
- **End frame:** Logs tab, live collector tail streaming.
- **Success criteria:** `localhost` / `127.0.0.1` endpoints clearly shown; live
  logs streaming. No external hostnames presented as the data destination.

### Section 12 — `reversible-revert`

- **Slug:** `reversible-revert`
- **Title:** Byte-for-byte reversible
- **Maps to:** pillars — `pillars.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Harnesses (enable -> disable), optional Mappings diff for the
  managed-region concept.
- **Preconditions/data state:** One harness in a known state (start disabled).
  Optionally have its config file open in a side tool to show the
  sentinel-bracketed managed region being added/removed (only if it can be
  shown without leaking a real username/path; otherwise rely on the in-app
  toggle + narration).
- **Steps:**
  1. Start on Harnesses tab with the chosen harness **disabled**.
  2. Wait 0.8 s. Click **Enable**. Hold 1.5 s as the managed region is written
     and the row turns green.
  3. (Optional, if a managed-region preview/diff is exposed in-app) open it and
     hold 2.0 s showing the sentinel-bracketed block.
  4. Click **Disable**. Hold 1.5 s as the row reverts and (if shown) the managed
     region is removed cleanly.
  5. Park cursor on the reverted row. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "Enable writes a sentinel-bracketed managed region."
  - 5.0 s: "One click reverts it, byte-for-byte."
  - 10.0 s: "No orphaned env vars. No half-applied state."
- **Start frame:** Harnesses row disabled.
- **End frame:** Same row cleanly reverted to disabled.
- **Success criteria:** A full enable -> disable round trip with the row
  returning to its exact original state. No real config path/username leaked.

### Section 13 — `native-otel`

- **Slug:** `native-otel`
- **Title:** Native OTel where it exists
- **Maps to:** pillars — `pillars.ts`
- **Target duration:** 10-14 s
- **Tab/screen:** Harnesses
- **Preconditions/data state:** Native OTel harnesses present (Claude Code,
  Gemini CLI, Codex CLI/desktop, Qwen Code, OpenCode, Cursor IDE) showing
  Telemetry on with no extra setup.
- **Steps:**
  1. Start on Harnesses tab. Wait 1.0 s.
  2. Hover 3 Native OTel rows in turn (e.g. Claude Code, Gemini CLI, Codex CLI),
     ~1.0 s each, so each shows Telemetry on and a clean detection subtitle.
  3. Enable one currently-disabled Native OTel harness; hold 1.5 s as it turns
     green instantly (Trove just flips the flag — no watcher).
  4. Park cursor. Hold end frame 1.0 s.
- **Captions / narration beats:**
  - 0.0 s: "Six harnesses speak OpenTelemetry natively."
  - 5.0 s: "Trove just flips the env or config flag."
  - 9.0 s: "Green instantly — no watcher needed."
- **Start frame:** Harnesses list filtered to Native OTel rows.
- **End frame:** A Native OTel row freshly enabled and green.
- **Success criteria:** 3+ Native OTel harnesses shown as telemetry-on; one
  enables instantly to green.

### Section 14 — `best-effort-adapter`

- **Slug:** `best-effort-adapter`
- **Title:** Best-effort adapters
- **Maps to:** pillars — `pillars.ts`
- **Target duration:** 12-16 s
- **Tab/screen:** Harnesses, then Mappings (hook rules)
- **Preconditions/data state:** Best-effort harnesses present (Cline, Aider,
  GitHub Copilot CLI) showing the **Best Effort** badge and Telemetry
  unknown/derived. Mappings has **hook rules** for at least one of them.
- **Steps:**
  1. Start on Harnesses tab. Wait 1.0 s. Scroll to the Best Effort rows
     (Cline / Aider / GitHub Copilot CLI). Hover each ~1.0 s so the **Best
     Effort** badge is legible.
  2. Enable one best-effort harness (e.g. Aider). Hold 2.0 s — note this
     installs a watcher / shell-rc wrapper rather than flipping a native flag.
  3. Click the **Mappings** tab and select that harness to reveal its **hook
     rules** (how watcher events become Tier A metrics). Hold 2.0 s.
  4. Park cursor. Hold end frame 1.0 s on the hook rules.
- **Captions / narration beats:**
  - 0.0 s: "Some tools don't emit OpenTelemetry at all."
  - 5.0 s: "Trove watches their logs and derives OTLP."
  - 10.0 s: "Hook rules classify those events into Tier A."
- **Start frame:** Harnesses list at the Best Effort rows.
- **End frame:** Mappings hook rules for a best-effort harness.
- **Success criteria:** Best Effort badge clearly shown; a best-effort harness
  enables; its hook rules are visible on the Mappings tab.

---

## 3. Screenshots (static, for the docs and site)

Static screenshots live in `packages/site/public/screenshots/` and are also
sourced from `documentation/screenshots/`.

### Already captured (done)

| Name            | Source                                               | Status |
| --------------- | ---------------------------------------------------- | ------ |
| `overview.png`  | first populated frame of `troveOverviewAnimated.gif` | done   |
| `harnesses.png` | `documentation/screenshots/harnesses.png`            | done   |
| `platforms.png` | `documentation/screenshots/platforms.png`            | done   |

> `overview.png` was extracted from the canonical Overview animation (the same
> asset the README uses) because there is no standalone static Overview capture.
> If you recapture it natively, prefer a real window capture per the spec below.

### TODO (capture natively)

| Name               | Tab / screen                                                           |
| ------------------ | ---------------------------------------------------------------------- |
| `mappings.png`     | Mappings tab (already exists in docs; recapture to spec if refreshing) |
| `logs.png`         | Logs tab — live collector tail                                         |
| `settings.png`     | Settings tab — auto-update + identity tagging                          |
| `setup-wizard.png` | First-run setup / connect-a-backend wizard                             |
| `first-launch.png` | First-launch empty/auto-detect state                                   |

### Capture instructions (apply to every screenshot)

1. **Theme:** Dark. Canvas `#1c1c1e`, Trove teal `#2dbfb8` accents.
2. **Resolution:** 2x Retina. Use a clean **window capture** so the rounded
   window chrome + shadow are included:
   - macOS: **Shift-Cmd-4**, then **Space**, then click the Trove window. This
     captures just the window at 2x with transparent corners.
3. **State:** Populate the screen the same way as the demo data setup (Section
   1.2) so it looks alive — enabled harnesses, configured platforms, recent
   signal, non-zero counters.
4. **Redaction (mandatory):** No real tokens / ingestion keys / API keys / Basic
   auth; no real emails (use `demo@trove.dev` / `Demo User`); no real org,
   tenant, or dataset names; no home-directory usernames in config paths
   (record under a demo account or crop/blur the path). `localhost:PORT`
   endpoints are fine.
5. **Crop/normalize:** Target output **~1080x1256** (portrait-ish window crop,
   matching the existing docs captures). Trim empty desktop; keep the window
   chrome.
6. **Output:** Save as PNG to
   `packages/site/public/screenshots/<name>.png`. Keep the source 2x capture in
   `documentation/screenshots/` if it is a new canonical asset.

#### Per-screenshot notes

- **`mappings.png`** — Show a harness's synthesis rules with the five Tier A
  targets visible and, if possible, the live diff preview open.
- **`logs.png`** — Capture mid-stream with several collector log lines visible;
  ensure no log line contains a real token or external hostname as the data
  destination. A `127.0.0.1` bind line is a good thing to include.
- **`settings.png`** — Show the auto-update control and the identity tagging
  fields (`user.name` / `user.email`) populated with demo values only.
- **`setup-wizard.png`** — Capture the connect-a-backend / first-run wizard step
  with an empty/placeholder credential form (never a populated real key field).
- **`first-launch.png`** — Capture the just-installed state: harnesses
  auto-detected, nothing enabled yet (the "before" of the `enable` clip).
