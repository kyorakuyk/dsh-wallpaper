# dsh-wallpaper-bridge

Companion DeepSeek Harness plugin for `dsh-wallpaper`. It exposes a versioned,
loopback-only REST/SSE surface backed by standard DSH agents and sessions.

The status route is intentionally public so the wallpaper can distinguish a
plain DSH web server from an installed bridge. All session routes require the
random bearer token stored in `$DSH_HOME/wallpaper/bridge-token` (or
`~/.dsh/wallpaper/bridge-token`). The token is consumed by the native wallpaper
process and must never be exposed to its WebView or logs.

## Desktop entry boundary

Wallpaper sessions receive a scoped `wallpaper:desktop-entry` system context.
It identifies the `桌面会话` workspace and active permission preset so the
model does not mistake the wallpaper composer for the full Harness Web UI.
New sessions default to the host's `workspace-write` permission preset and
dedicated workspace directory. Resumed sessions keep the user's explicit
permission choice. Tool approval that cannot be handled in the wallpaper is
handed off to Harness.

The host may override the initial preset with `desktopPermission`. This applies
only when creating a new desktop session; the wallpaper control surface remains
the place where the user changes the active session permission afterward.

The bridge registers no status, control, or session route unless the host web
server is bound to the exact loopback address `127.0.0.1`. Every new session is
attached to the dedicated desktop workspace, and `resumeSessionId` is accepted
only for a session already owned by that workspace; unknown or foreign IDs
return the same `resume-unavailable` response without disclosing ownership.

## Version boundaries

Two independent versions meet here, and confusing them is what previously made
an installed copy impossible to identify:

| Value | Meaning | Where it comes from |
| --- | --- | --- |
| `protocolVersion` (`1`) | the wallpaper↔Bridge REST/SSE contract | `BRIDGE_PROTOCOL_VERSION` |
| `bridgeVersion` | this Bridge's release | `bridge/package.json` `version` |
| `bridgeBuild` | non-sensitive build provenance (`dev` for a local build) | `DSH_WALLPAPER_BRIDGE_BUILD` |
| `authoredAgainst` | the DSH API range this build was compiled against | `bridge/package.json` peers |

`protocolVersion` is the only value the wallpaper gates on. A wallpaper built
against protocol 1 keeps working with any newer Bridge; a breaking change needs
`/api/wallpaper/v2` so v1 has a compatibility window. `bridgeVersion` and
`bridgeBuild` exist so support can tell a stale profile copy from a current one
without reading `node_modules`.

**There is no host-version field, on purpose.** DSH exposes no version at
runtime — no `DSH_VERSION` environment variable and no version service, only a
`dsh --version` CLI flag that a loaded plugin cannot read. An earlier draft of
this status reported `hostVersion`/`hostVerified`; that would always have been
`unknown`, so it was removed rather than shipped as a field that looks like a
check but never performs one. `authoredAgainst` is what the code can actually
prove, and it is the value the matrix below is keyed on.

## DSH compatibility matrix

| DSH host | Compiled against | Verified at runtime | Notes |
| --- | --- | --- | --- |
| `0.1.0-rc.5` | satisfy the `^0.1.0-rc.5` range | **yes** — `bridge/tests/realDshSmoke.spec.ts` | The version installed on the development machine. All seven injected services compose, `state` is `bridge-ready`, and create/resume/history/cancel/SSE were exercised over HTTP. |
| `0.1.0-rc.6` | compiled and typechecked against rc.6 definitions | **no** | No rc.6 host is installed on the development machine, so nothing was measured at runtime. The declared range admits it, but that is a range, not a result. |

`peerDependencies` declares `^0.1.0-rc.5` for the DSH packages. That range is
deliberately wider than the definitions the compiler currently resolves (rc.6,
the newest satisfying release): under semver prerelease rules `^0.1.0-rc.6` does
**not** admit `0.1.0-rc.5`, so the previous declaration excluded the one host
this Bridge is proven to run on. Do not narrow it back without first re-running
the smoke test against the version being excluded, and do not add a row to the
matrix above that no test has measured.

### Re-verifying after a DSH update

1. `pnpm -C bridge build` and `pnpm -C bridge typecheck` against the new
   definitions.
2. Confirm the compiled-against range is the one you intend: `bridgeVersion` and
   `authoredAgainst` in the status response are read from `package.json`, so they
   change with `pnpm install` rather than with a source edit.
3. `pnpm -C bridge exec vitest run tests/realDshSmoke.spec.ts` with
   `DSH_WALLPAPER_SMOKE_DSH_ROOT` (and optionally
   `DSH_WALLPAPER_SMOKE_PROFILE`) pointed at the new checkout. The test clones
   the profile into a throwaway `DSH_HOME`, so it does not touch the real
   profile, sessions, or credentials.
4. Only then add the measured version to the matrix and, if appropriate, widen
   the peer range.

## Host adapter

All host access goes through `bridge/src/host.ts`. It validates the consumed
service surface once, when the scope composes, and every route handler receives
the validated adapter instead of re-asserting service shapes with `unknown as`.
A host missing a member the Bridge drives yields
`state: "bridge-incompatible"`, `reasonCode: "host-shape-mismatch"` on
`/status`, naming the member; the status route stays mounted so the condition is
diagnosable, and no session route is announced for a host that cannot serve one.

Two details are deliberate and easy to get wrong:

- **Event subscription** happens through `ctx.on` inside `apply()`, not through
  the adapter. `apply()` runs before any service scope exists, and subscribing
  there is what lets the Bridge observe sessions a host creates outside these
  routes.
- **`inject` is documentation, not the gate.** This module has a default export
  (the `apply` function). The Cordis loader unwraps `exports.default ?? exports`
  before reading `plugin.inject`, so the module-level `inject` list never
  reaches the fiber; `apply()` gates its own work with explicit `ctx.inject`
  calls instead. That is also what allows the public `/status` route to mount
  before the session services are composed, which is how the wallpaper observes
  `state: "bridge-loading"` instead of "no Bridge".

## Updating the installed Bridge

A copy under a DSH profile's `node_modules` is a **copy**, not a link to this
repository (the desktop profile uses pnpm's hoisted linker), so editing
`bridge/src` does not change what a running DSH loads. After changing this
package, refresh the profile:

```powershell
dsh plugin --profile desktop install   # or the profile you run
```

Then confirm from the status response that `bridgeVersion`/`bridgeBuild` changed
rather than trusting the local build output. The wallpaper never runs package
management on its own; updating a profile is a user action.

The procedure was verified end to end in a throwaway profile, and the result was
then confirmed against a real running DSH rather than only by hashing files: with
the refreshed profile, `/api/wallpaper/v1/status` returned the new contract —

```json
{"bridgeVersion":"0.1.1","bridgeBuild":"dev","protocolVersion":1,"dsh":"online",
 "authoredAgainst":"^0.1.0-rc.5","state":"bridge-ready","reasonCode":"ready",
 "capabilities":["status","control","sessions","history","sse","cancel",
                 "approval-handoff","resume"],"authentication":"ready"}
```

Before the refresh that same endpoint returned only
`bridgeVersion/protocolVersion/dsh/capabilities/authentication` — no `state`, no
`reasonCode`, no `authoredAgainst`, and a `bridgeVersion` of `1.1.0`. So the
refresh is what makes the new diagnostics observable, and it is also how a stale
copy is told apart from a current one.

Three things that will bite an automated or relocated attempt:

- **pnpm aborts without a TTY.** If pnpm decides the modules directory must be
  recreated it fails with
  `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY`. Run it in a terminal, or set
  `CI=true` for the command.
- **A plain install may not refresh the copy.** When the `file:` resolution has
  not changed, pnpm considers the dependency satisfied and leaves the installed
  copy alone — measured: the first attempt left the old hash in place. Use
  `pnpm install --force` (or `pnpm update dsh-wallpaper-bridge`) when the profile
  copy must actually move to the current build, then confirm the hash or the
  status response changed rather than assuming it did.
- **`file:` dependencies resolve relative to `$DSH_HOME\profiles`, not to the
  profile.** This profile's spec is `file:C:/DeepSeekHarness/plugins/...`, an
  absolute path that also works as a relative one from `$DSH_HOME\profiles`.
  A profile created anywhere else must therefore keep the `file:` target
  reachable from that location, or the install fails with `ENOENT` on a path
  built out of the home directory. Do not "fix" this by rewriting the `C:`
  junction: it is valid, and the resolution depends on that spelling.


## Status states

| `state` | Meaning | User action |
| --- | --- | --- |
| `bridge-ready` | every required route is mounted and the token is available | none |
| `bridge-loading` | the Bridge is mounted, its session services are still composing | wait |
| `bridge-auth-unavailable` | the Bridge is mounted but its bearer token is unavailable | restart the wallpaper; check token directory ACLs |
| `bridge-incompatible` | protocol version, capability set, or host shape does not match | update DSH or the Bridge |

`capabilities` lists only routes that are actually registered, so
`bridge-ready` implies `POST /sessions` will not 404. `resume` appears only when
the host exposes session persistence.
