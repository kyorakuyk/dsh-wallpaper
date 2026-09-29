# 把 Bridge 发到 npm（独立 GitHub 仓库 + Action 自动发布）

记录时间：2026-09-30。目的：让用户**在官壳自己的「添加插件」对话框里填一个包名**就能装上桥
（那个对话框接受"包名 / GitHub 仓库地址 / 本地目录路径"三种，而它自己有权写 profile —— CLI 对 `desktop`
一律拒绝，所以这条路是唯一顺畅的）。

已实测的前置事实：

* `dsh-wallpaper-bridge` 这个包名在 npm 上**未被占用**（`npm view` 报 404）。它正好就是
  `bridge/package.json` 里的 `name`，所以**不需要改名**；`@deepseek-ai/…` 虽然也空着，但那是别人家的
  scope，发不进去。
* 从**本地目录**装（`dsh plugin --profile web add <目录>`）可用，而且它会自己完成两件事：
  `dsh: initialized profile web at …`（缺失的档案自动初始化）、把包名写进 `dsh.profile.bundles`。
* 但**安装不会编译**：拿一份没有 `lib/` 的副本去装，源目录与部署副本都仍然没有 `lib/` ⇒ 插件加载不起来。
  所以发布物必须自带可加载的 `lib/`，而 `bridge/lib/` 在本仓库是 `.gitignore` 掉的 ⇒ **由发布流程构建**
  （见下面的 Action，它显式 `pnpm build` 之后再发）。

---

## 一、在新仓库里放什么

把本仓库 `bridge/` 的内容搬过去（它是一个自洽的包：源码、清单、补丁与测试都在里面）：

```
package.json          cordis.patch.yml     tsdown.config.ts
tsconfig.json         src/                 tests/            README.md
LICENSE               .gitignore  (node_modules/、lib/)
```

`package.json` 需要补几个字段（作为公开包）：

```jsonc
{
  "repository": { "type": "git", "url": "https://github.com/<你>/dsh-wallpaper-bridge.git" },
  "homepage": "https://github.com/<你>/dsh-wallpaper-bridge",
  "bugs": { "url": "https://github.com/<你>/dsh-wallpaper-bridge/issues" },
  "keywords": ["dsh", "dsh-plugin", "deepseek-harness", "wallpaper"],
  "engines": { "node": ">=22" },
  "publishConfig": { "access": "public" },
  "license": "MIT"
}
```

已经有、**不要动**的两处：`scripts.prepare = tsdown`（git 依赖安装时会用到它）与
`files: ["lib", "cordis.patch.yml", "README.md"]`（决定发布物里有什么）。

另外给新仓库加上 GitHub 话题 **`dsh-plugin`** —— DSH 的 `CONTRIBUTING.md` 明确说插件通过这个话题被发现。

## 二、一次性准备（2026-09-30 实测过的路径）

**先看结论**：npm 对新账号/新界面**已经不提供 Classic token**（点 "Generate New Token" 直接进
Granular 表单）。所以要走的是 **Granular + 勾 Bypass 2FA + `publish and stage`**：

1. npm → Access Tokens → Generate New Token（Granular）：
   * 勾 **`Bypass two-factor authentication (2FA)`** —— 不勾的话 CI 会以
     `ERR_PNPM_OTP_NON_INTERACTIVE` 失败（发布要求一次性验证码，而 CI 里没人能输）；
   * Packages and scopes → Permissions 选 **`Read and write (publish and stage)`** ——
     选成 `stage only` 会以 `403 … This token can only publish to a staging area` 失败（那一档只能发到
     暂存区，要走 `npm stage publish` 的人工晋级流程）；
   * Organizations → `No access`（发的是无 scope 的公开包）；期限按需；
2. 把 token 写进新仓库的 secret：`gh secret set NPM_TOKEN -R <owner>/<repo>`（粘贴时隐藏）；
   本机**不需要**登录 npm —— 发布由 Action 完成。

失败信息是递进的，认准这三条就能自己排：

| 报错 | 含义 |
| --- | --- |
| `[E404] 404 Not Found - PUT …` | 没带 token 或 token 无权限（npm 对"无权创建新包"故意返回 404） |
| `ERR_PNPM_OTP_NON_INTERACTIVE` | token 有效但没勾 Bypass 2FA |
| `[E403] … This token can only publish to a staging area` | 权限档位选成了 `stage only` |

**将来的事**：npm 已公告从 **2027 年 1 月**起限制"绕过 2FA 的 token 用于直接发布"（登录页横幅也写着）。
包建起来之后应切到 **Trusted Publishing（OIDC，免 token）**：在 npm 该包的设置里登记
"GitHub 仓库 + 工作流文件名（`publish.yml`）"，然后从工作流里删掉 `NODE_AUTH_TOKEN` —— pnpm 会自己走
OIDC（日志里那条 `Skipped OIDC: ERR_PNPM_AUTH_TOKEN_EXCHANGE` 就是这条路，只是它**要求包已存在**，
所以首发必须先靠 token）。

## 三、Action（`.github/workflows/publish.yml`）

```yaml
name: Publish Bridge

on:
  push:
    tags: ['v*']
  workflow_dispatch:

permissions:
  contents: read
  id-token: write          # npm provenance 需要它

jobs:
  publish:
    runs-on: windows-latest      # 见下方说明：桥是 Windows 专属的
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 11
      - uses: actions/setup-node@v4
        with:
          node-version: '22'
          registry-url: 'https://registry.npmjs.org'
          cache: 'pnpm'
      - run: pnpm install --frozen-lockfile
      - run: pnpm typecheck
      - run: pnpm test
      - run: pnpm build                       # lib/ 是不入库的构建产物，必须在发布前构建
      - name: 标签必须等于包版本
        if: startsWith(github.ref, 'refs/tags/')
        run: |
          tag="${GITHUB_REF#refs/tags/v}"
          pkg="$(node -p "require('./package.json').version")"
          if [ "$tag" != "$pkg" ]; then
            echo "标签 v$tag 与 package.json 的 $pkg 不一致：包版本必须与标签一一对应" >&2
            exit 1
          fi
      - name: Publish
        run: pnpm publish --no-git-checks --provenance
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
```

要点：

* **标签必须等于包版本**：这条守卫与壁纸那边 MSIX 的标签约定是同一个教训（"标签无法比较大小"曾经真的
  害过我们一次），宁可让发布失败，也不要发出一个版本号说不清的包；
* `--no-git-checks`：Actions 的检出是游离头，pnpm 默认会因此拒绝发布；
* `--provenance`：让 npm 页面上出现"由哪个仓库的哪次构建产出"的证明（这就是 `id-token: write` 的用途）；
* **runner 必须是 Windows**：第一版工作流用了 `ubuntu-latest`，结果是真实失败 —— 桥用
  `whoami.exe`/`icacls.exe` 收紧令牌目录的 ACL，测试也断言这些调用与 Windows 路径（`C:\Users\…`）。
  这不是可移植性缺陷，而是"桥服务于 Windows 壁纸"这一事实，所以 runner 与目标平台一致（与壁纸仓库的
  CI 相同）。另外那段"标签必须等于版本"的脚本是 bash 语法，在 Windows runner 上要显式写 `shell: bash`
  （默认是 pwsh）；
* 先 `build` 再 `publish`：**不要依赖 `prepare`** —— 它在 `npm publish` 时会跑，但把构建放在流程里显式
  可见，出问题时日志说得清。

## 四、发布一次

```bash
git tag v0.1.4 && git push origin v0.1.4      # 标签名 = v + package.json 里的版本
```

发布后核对（2026-09-30 首次发布实际跑过的四步）：

```bash
npm view dsh-wallpaper-bridge version                    # 等于刚推的标签
npm view dsh-wallpaper-bridge dist.attestations --json   # 有内容 = provenance 生效
npm pack dsh-wallpaper-bridge                            # 下 tarball
tar -tzf dsh-wallpaper-bridge-*.tgz                      # 里面必须有 package/lib/index.js
```

最后一条是**唯一**能证明"发布物自带构建结果"的证据：`lib/` 不入库，安装过程也不会编译，所以
tarball 里没有 `lib/` 的包装上去就是加载不起来的插件。

装一次（用户的姿势，建议用隔离 home 试）：

```bash
DSH_HOME=<临时目录> dsh plugin --profile web add dsh-wallpaper-bridge
```

预期：依赖写成 `dsh-wallpaper-bridge=^0.1.3`、`dsh.profile.bundles` 里出现它、部署副本里有 `lib/`。
（pnpm 会对刚发布的包记一条 `minimumReleaseAgeExclude` —— 那是它的"新版本冷却期"安全机制，正常现象。）

## 五、用户怎么装（写进 README 与壁纸的提示文案）

1. 打开官壳的「添加插件」，在输入框里填 **`dsh-wallpaper-bridge`**，安装源按需选镜像；
2. 装完**重启壳**（插件在启动时装载）；
3. 验证：`/api/wallpaper/v1/status` 返回 200 且 `state` 为 `bridge-ready`（壁纸「连接」页同时会亮）。

**升级＝先卸载再安装新版**——这是官壳自己那句提示（"插件安装后，暂不支持自动更新。若需升级，请先卸载
再安装新版"）。壁纸侧不要承诺自动更新；它现有的 `bridge-incompatible` 提示改成"卸载后重新安装"即可。

## 六、两条已知的坑

1. **本地目录安装用的是 `link:`**（实测：`pnpm add <目录>` 记成 `link:…`）⇒ profile 指向你 clone 的那个
   目录，删了/移了就坏。**发到 npm 才是稳妥路线**（npm 是拷贝，不是链接）。
2. **不要指望安装过程编译**（实测：没有 `lib/` 的副本装进去仍然没有 `lib/`）⇒ 发布物必须自带构建结果。

## 七、还有一处必须跟着搬的东西

`tests/hostCompatibility.spec.ts`（本仓库 `bridge/tests/` 里）钉着"peer 范围必须覆盖到哪一代"——它是
**跟着桥走的**，搬到新仓库后要保留：它读的是桥自己的 `package.json`，与宿主版本变更直接相关（0.2.0-rc.2
就是靠这条范围自动覆盖的，实测在真壳上 `bridge-ready`）。
