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

## Next

B7 moves `DESKTOP_FOREGROUND_STATE`, `start_foreground_monitor`, and foreground-surface classification helpers into the existing `foreground.rs`, preserving the J.4 boundary. `workspace` and `workerw` remain deferred to later batches.
