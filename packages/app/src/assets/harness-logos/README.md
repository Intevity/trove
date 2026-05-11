# Harness logo assets

Drop one SVG per harness here, named `<HarnessId>.svg` (e.g. `claude-code.svg`,
`gemini-cli.svg`, `cursor-ide.svg`). `HarnessList.tsx` picks them up at build
time via `import.meta.glob` and renders them in place of the default monogram
tile for each row. Any harness ID without a matching file keeps the monogram
fallback.

Conventions:

- Transparent background (the surrounding row supplies the surface color).
- Square viewBox; the element is rendered at 32×32 px.
- Keep the file under a few KB — these are part of the JS bundle.
- File names must match the `HarnessId` discriminant in
  `packages/shared/src/schemas.ts` exactly.

Logo sourcing is intentionally left to whoever is shipping the build. Each
harness vendor publishes its own brand guidelines (e.g. Anthropic's brand
page, Google's brand permissions, GitHub's logo usage, Cursor's press kit);
follow those before including any third-party mark.
