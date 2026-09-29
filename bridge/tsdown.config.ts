import { execFileSync } from 'node:child_process'

/**
 * Which build this is, baked into the bundle at compile time.
 *
 * `bridgeVersion` says which *release* a copy is, and that was not enough: three
 * copies in three profiles all reported `0.1.3` while containing three different
 * builds. So the build stamps itself, and `status` reports it — one field that
 * answers "is this profile still running the copy I just published".
 *
 * Order, and each step is honest about what it knows:
 *   1. `DSH_WALLPAPER_BRIDGE_BUILD` — the release workflow sets this (tag or sha);
 *   2. otherwise `dev+g<short sha>` when the build runs inside a git worktree;
 *   3. otherwise plain `dev`.
 *
 * Nothing here may fail the build: a worktree without git, or a git that refuses
 * to answer, simply means the stamp says less.
 */
function buildStamp(): string {
  const declared = process.env.DSH_WALLPAPER_BRIDGE_BUILD?.trim()
  if (declared) return declared.slice(0, 40)
  try {
    const sha = execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
    if (/^[0-9a-f]{4,40}$/i.test(sha)) return `dev+g${sha}`
  } catch {
    /* not a git worktree, or no git: the stamp says less, which is the truth */
  }
  return 'dev'
}

export default {
  name: 'dsh-wallpaper-bridge',
  entry: { index: 'src/index.ts', protocol: 'src/protocol.ts' },
  outDir: 'lib',
  format: ['esm'],
  platform: 'node',
  target: 'es2022',
  fixedExtension: false,
  dts: false,
  clean: true,
  // `BRIDGE_BUILD` 读的就是这个名字，替换之后它变成字符串字面量（源码里用的是普通成员访问，
  // 不是可选链，正是为了让这条替换对得上）。
  define: { 'process.env.DSH_WALLPAPER_BRIDGE_BUILD': JSON.stringify(buildStamp()) },
}
