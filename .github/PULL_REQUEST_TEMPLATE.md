## Summary

<!-- 1–3 bullet points: what changes and why. Lead with the user-visible
     impact, not the implementation detail. -->

-

## Type

- [ ] Bug fix
- [ ] New feature
- [ ] Adapter (new harness)
- [ ] Backend (new preset)
- [ ] Refactor / internal
- [ ] Documentation
- [ ] CI / build

## Related issue

<!-- e.g. "Closes #123" or "Refs #456". Leave blank if standalone. -->

## Tests

<!-- What's covered by automated tests, and what was verified manually.
     UI changes: please include a screenshot or short clip. -->

- [ ] Unit / integration tests added or updated
- [ ] Manually verified end-to-end (describe how)

## Checklist

- [ ] `pnpm lint` clean
- [ ] `pnpm typecheck` clean
- [ ] `pnpm format:check` clean
- [ ] `pnpm test` green (95% coverage threshold preserved)
- [ ] `cargo clippy --all-targets -- -D warnings` clean (if Rust changed)
- [ ] Conventional Commits in the title (e.g. `feat(adapters): …`)
- [ ] Touched docs (`README.md`, `documentation/*.md`) if user-visible behaviour changed

🤖 Generated with [Claude Code](https://claude.com/claude-code)
