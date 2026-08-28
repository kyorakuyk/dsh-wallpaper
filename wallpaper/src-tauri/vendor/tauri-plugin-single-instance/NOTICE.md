# Local vendor notice: `tauri-plugin-single-instance`

This directory is a deliberately small, auditable vendor copy of
[`tauri-plugin-single-instance` 2.4.3](https://crates.io/crates/tauri-plugin-single-instance/2.4.3),
from Tauri's `plugins-workspace` repository.

- Upstream crate version: `2.4.3`
- Upstream source revision recorded by Cargo: `cad301fcc1f3ebad1eaef552c886b0bc8580c3fe`
- Upstream repository: <https://github.com/tauri-apps/plugins-workspace>
- Original license: Apache-2.0 OR MIT (both full license texts are retained
  as `LICENSE_APACHE-2.0` and `LICENSE_MIT`; SPDX metadata is retained as
  `LICENSE.spdx`).

Local modification:

- `src/platform_impl/windows.rs` hardens the named-mutex to IPC-window startup
  race. A second launch waits for the primary IPC target or obtains the mutex
  only after the prior owner exits. It fails closed after a bounded timeout,
  uses bounded IPC forwarding, and closes native handles on every path.

No application-specific behavior is added to the vendored crate.
