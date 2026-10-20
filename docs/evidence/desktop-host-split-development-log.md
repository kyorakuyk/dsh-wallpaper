# Desktop host split — development log

## B6 — focus extraction

- Extracted focus tracing, focus handover verification, the island-click check, keyboard-focus restoration, and the three focus-classification tests into `wallpaper/src-tauri/src/desktop_host/foreground.rs`. The foreground monitor remains in `mod.rs` for B7. Preserved the `windows_integration` facade exports and changed only required cross-module visibility/imports.
- Frozen base: `4e60bf8c2d70b1bc10a36fd44fa10c1e98e762f9`. Reviewed tree: `70bfa7a215374941f77e9da8ea60d52e4652f631`. Commit: `6de0496a58949e4e963627c841df4eb756b8eba1`; local tag `checkpoint/desktop-host-b6-focus` points to that commit.
- Structural checks against the frozen base passed: moved focus/restore bodies and all three tests match; monitor body is unchanged. `SAFETY` comments: 99; unsafe blocks: 99; test attributes: 45; non-Windows cfgs: 18 direct + 2 combined, all unchanged.
- Local Rust gates passed: root focused tests 17/17 and foreground tests 3/3; Windows all-target library 407 passed/14 ignored plus 48 integration tests; Lite library 124 passed/2 ignored; full/Lite debug and release checks; undocumented-unsafe clippy. Frontend: 604 passed; bridge: 96 passed/1 skipped; both typechecks and builds passed; Lite bundle boundary passed (4 web files, 5 required resources); `git diff --check` passed.
- Rust warning summaries matched B5 CI: full library 44, full library tests 40; integration targets 5/80/28/50/53; Lite library 146, Lite library tests 70; `tauri-plugin-single-instance` 1. No increase was observed.
- Exact staged review fingerprint: SHA-256 `eeb543965dc160f7b99b58f67506f24bab6b8f087876a10cc5b7d9ca6b36693a`, 61,636 bytes. Both independent reviews reported no security or logic findings. One marked candidate-scope cleanliness true; the other noted the preserved, unrelated untracked `docs/blog/` directory while also confirming there was no unstaged tracked diff.
- Push CI run `39490998347` completed successfully (Rust full, Lite, Frontend/bridge) on `6de0496a58949e4e963627c841df4eb756b8eba1`; remote branch read-back matched. The checkpoint tag remains local.
- Not verified: Windows GUI acceptance and user-only checks in the task book. A Linux cargo check attempt stopped in `libdbus-sys` setup before compiling the project crate, so it provides no cross-platform result. Existing untracked `docs/blog/` files were excluded and untouched.

## B7 — foreground monitor extraction

- Moved `DESKTOP_FOREGROUND_STATE`, its getter, desktop foreground-class predicate, foreground monitor (including the non-Windows stub), surface predicates, and the two associated existing tests into the existing `foreground.rs`. `mod.rs` retains shared watcher helpers and the workspace monitor, with explicit facade re-exports for existing callers.
- Frozen base: `bb5724f88b1a366e50bbce241b115f4f15f833a6`. Reviewed index tree: `9a67a014e7d995c5575c31833a07b26916b2ae17`; diff SHA-256 `aafa018efc60ba87c69f1db045b59656663dc0614740273dddb9b195244d8ff1`, 22,841 bytes. Two independent exact-snapshot reviews passed with no security or logic findings. The first review caught separator whitespace drift; the three baseline blank lines were restored and both reviews were rerun against the corrected fingerprint. A suggestion to cfg-gate one test-only import is non-blocking because non-Windows test builds are not an established target.
- Commit `e5338583183f35a5ffd6d82c2a635b9ad2bd06b1` has the reviewed tree; local tag `checkpoint/desktop-host-b7-foreground` points to it. Push read-back matched the local commit.
- Structural checks: moved implementations/comments/cfgs/tests match the baseline; shared helpers stay in the parent; `SAFETY` comments 99, unsafe blocks 99, test attributes 45, non-Windows cfgs 18 direct + 2 combined, all unchanged. Resulting module sizes: `mod.rs` 1,997 lines, `foreground.rs` 733 lines.
- Local Rust gates passed: focused parent tests 15/15 and foreground tests 5/5; Windows all-target suite 407 passed/14 ignored plus 48 integration tests; Lite suite 124 passed/2 ignored; Full/Lite debug and release checks; undocumented-unsafe clippy. Frontend: 604 passed; bridge: 96 passed/1 skipped; typechecks, both builds, Lite bundle boundary (4 web files, 5 required resources), and diff checks passed.
- Warning summaries matched B6: Full library 44, Full lib tests 40; integration targets 5/80/28/50/53; Lite library 146, Lite lib tests 70; `tauri-plugin-single-instance` 1. No increase was observed.
- GitHub CI `39500317959` completed successfully at `2026-10-20T21:30:59Z` on `e5338583183f35a5ffd6d82c2a635b9ad2bd06b1` (Rust full, Lite, Frontend/bridge). Workflow annotations report Node.js 20 actions being forced to Node.js 24; all jobs passed.
- Not verified: Windows GUI acceptance and user-only checks in the task book. Existing untracked `docs/blog/` remains excluded and untouched.

## Next

B8 extracts the workspace owner described in report-r3 J.1/J.4 into the new `workspace.rs`; keep `workerw` deferred to its later batch. The five cross-group state statics remain in `state.rs` as already completed in B5a.
