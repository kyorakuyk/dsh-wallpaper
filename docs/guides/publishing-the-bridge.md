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

## 二、一次性准备

1. 在 npm 生成一个 **Automation** 类型的 token（Classic token 里选 Automation，或 Granular 里给
   目标包 Read and write）；
2. 在新仓库的 Settings → Secrets and variables → Actions 里加一个 secret：`NPM_TOKEN`；
3. 本机**不需要**登录 npm —— 发布由 Action 完成。

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
    runs-on: ubuntu-latest
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
* 先 `build` 再 `publish`：**不要依赖 `prepare`** —— 它在 `npm publish` 时会跑，但把构建放在流程里显式
  可见，出问题时日志说得清。

## 四、发布一次

```bash
git tag v0.1.4 && git push origin v0.1.4      # 标签名 = v + package.json 里的版本
```

发布后核对：

```bash
npm view dsh-wallpaper-bridge version         # 应当等于刚推的标签
npm view dsh-wallpaper-bridge dist.tarball    # 可以下载下来确认里面有 lib/index.js
```

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
