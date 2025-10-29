## Codex TUI Customization Playbook

This repository keeps the upstream OpenAI TUI as the "engine" and applies the bespoke Codex skin as a thin layer on top. The goals are:

1. Keep upstream merges straightforward.
2. Make the custom statusline opt-in.
3. Guard behaviour with targeted tests and snapshots.

### 1. Layer, Don’t Fork

- **Renderer seam only.** Leave upstream widgets intact. The custom look lives in `codex-rs/tui/src/statusline/skins`, implementing `StatusLineRenderer`. Upstream logic keeps producing `StatusLineSnapshot`; the renderer turns that snapshot into our palette and capsule.
- **Config flag.** The renderer activates when `Config.tui_custom_statusline` is `true` (default). If the flag is disabled the upstream bar renders unchanged. Avoid direct feature gates inside upstream modules—keep the hook isolated to `ChatWidget`.
- **Custom assets in one module.** Colors, icons, and layout helpers stay in the statusline skin module. Nothing outside the module should rely on our palette to minimize conflict surface during merges.

### 2. Automate Reapplication

- **Patch stack.** Maintain the customization as a small patch series (e.g. `git format-patch` or `git rerere`). Each release: pull upstream, replay the patch stack, resolve any new trait mismatches.
- **Scripted apply.** Optional helper `scripts/apply-customizations.sh` can replay the patches against a fresh checkout to guarantee deterministic diffs.
- **Reuse conflict knowledge.** Keep `git rerere` enabled. Once a merge conflict in the renderer hook is resolved, rerere remembers it for the next upstream sync.

### 3. Strengthen Tests

- **Statusline snapshots.** `statusline/tests.rs` and `chatwidget/tests.rs` cover both the upstream and custom capsules (queued messages, hints, timers, git/kube metadata). When the renderer changes, update or extend these snapshots instead of loosening assertions.
- **Run pill parity.** Tests such as `custom_renderer_matches_default_run_pill` ensure that our skin mirrors upstream semantics unless the flag is set.
- **VT100 coverage.** Snapshot tests that render the entire layout (history + custom statusline) act as regression guards for spacing and hint placement.

### Workflow Checklist

1. `git pull upstream main`.
2. Reapply customization patches (`scripts/apply-customizations.sh`).
3. Resolve new compiler or trait changes in the renderer.
4. `just fmt`, `just fix -p codex-tui`, `cargo test -p codex-tui`.
5. Update snapshots only when behaviour intentionally changes.

With this structure, upstream refreshes become “pull → reapply → fix hooks” instead of wrestling with widespread conflicts.

