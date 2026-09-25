# dsh-wallpaper 仓库恢复方案

> 撰写时间：2026-09-22 21:30
> 起因：本地 `.git` 在 2026-09-22 21:04 被一次中断的后台 `git gc` 破坏。
> **执行前请通读全文。第 0 节是硬性前置条件，不做它后面每一步都可能把损失扩大。**

---

## 0. 硬性前置条件（先做这个，否则不要动）

### 0.1 所有 git 命令必须显式禁用自动 gc

```bash
# 本文后续所有 `$G` 都指代这一串，请不要省略
G="git -c gc.auto=0 -c gc.autoDetach=false"
```

**为什么**：这次事故的根因就是 `gc.autoDetach` 默认 `true` —— 读命令（`git status` / `git log` / `git diff`）也会触发 `gc --auto`，而 gc 会 fork 到后台继续跑；前台命令返回后宿主回收残留子进程，后台 gc 被腰斩在打包中途。结果是 `pack-refs --prune` 已删掉 loose refs、但 `packed-refs` 还没写成 → `.git/refs` 全空 → git 对所有命令报 `not a git repository`。

### 0.2 永久写死（在旧仓库上）

因为 git 已经不认这个仓库，只能用文本编辑器直接改 `.git/config`，追加：

```ini
[gc]
	auto = 0
	autoDetach = false
```

### 0.3 确认两个备份都在

| 备份 | 路径 | 规模 |
|---|---|---|
| `.git` 全量 | `D:\Family\DeepSeekHarness\plugins\.git-backup-dsh-wallpaper-20260922` | 36 文件 / 82.51 MB |
| 工作区全量 | `D:\Family\DeepSeekHarness\plugins\_dsh-wallpaper-workspace-20260922` | 382 文件 / 139.87 MB |

**这两个目录是回滚点，在恢复完成并验证通过之前不要删除、不要移动。**

### 0.4 确认没有别的进程在用这个仓库

**如果有其他 agent（例如 Luna）以这个目录为工作目录，先让它停下来。** 恢复过程会替换目录，在途改动会丢。

---

## 1. 损失范围（实测确认，供核对）

### 已丢失

| 项 | 详情 |
|---|---|
| `.git/refs` + `.git/packed-refs` | 全丢（**可从 reflog 重建**） |
| `pack-*.pack` | 被截断。`git verify-pack -v` 仍报 `ok`（它只校验结构），但 `git fsck` 出现大量 `failed to load pack entry` |
| 3 个未推送提交的对象 | `c89d65b` / `d1b90cd` / `c6d8e83`（2026-09-18 的 CapsuleBubble 三次提交）→ **永久丢失，本地无副本、远程未推送、全盘无第二个 clone** |
| 一批 blob | 含 `.git/index` 的 cache-tree 对象 `ca74ccf`（即 `.git/AUTO_MERGE` 的值） |

### 完好

| 项 | 详情 |
|---|---|
| **工作区全部文件** | 382 文件 / 139.87 MB，含 `docs/`、`.workbuddy/`、`wallpaper/src`、`bridge/` |
| **`origin`（GitHub）** | master + `codex/startup-bootstrap-retry` + `codex/v0.3-rewrite` 三个分支 |
| **`5f5e4de` / `3800601`** | 对象仍在且**可完整读出**（`git cat-file -p 5f5e4de` 成功，`git ls-tree -r 5f5e4de` 能列出 `.workbuddy/` 全部 11 个文件） |
| `.git/logs/`（reflog） | 完好 → 所有分支 tip 的 sha 可查（**refs 重建依据**） |
| `.git/HEAD` / `index` / `COMMIT_EDITMSG` / `ORIG_HEAD` | 完好 |

### 从 reflog 读出的引用（重建用）

```
refs/heads/master                                 = d1bd594536d99aa39c0b2cfd4ab2c5f87cd59454
refs/heads/codex/v0.3-rewrite                     = 156c2ecda1e6fc47ddb012eef03bb01a9f61cee0
refs/heads/codex/startup-bootstrap-retry          = c6d8e8307d36a0b128439b5d569cbea066b78c2f   ← 对象已丢
refs/remotes/origin/master                        = d1bd594536d99aa39c0b2cfd4ab2c5f87cd59454
refs/remotes/origin/codex/v0.3-rewrite            = 156c2ecda1e6fc47ddb012eef03bb01a9f61cee0
refs/remotes/origin/codex/startup-bootstrap-retry = f4907ee7070b856e1805467b897e07906f9985ba
refs/remotes/origin/HEAD                          = ref: refs/remotes/origin/master
```

可抢救的本地独有提交：`5f5e4de64d697d540e287b47977d8c86b8677c97`、`38006013b9955b28a28487e83564fa50cb45425d`

---

## 2. 关键判断：选哪条路

### 先说结论

**推荐「方案 A」**：新 clone + 工作区覆盖 + 一个恢复提交。

理由：丢失的那 3 个提交**内容全在工作区里**（工作区就是 `c6d8e83` 检出后的状态，且 `c6d8e83` 本身是一次 revert，最终净效果已经反映在工作区）。**代码一行不丢**，丢的只是提交颗粒度 —— 而这 5 个未推送提交都是 AI 批量作业产生的 checkpoint（`Checkpoint: Save current state...` / `Fix: ...`），保留价值的颗粒度很低。

方案 B 能把 `5f5e4de` / `3800601` 的提交信息救回来，但要动 `filter-branch`，步骤多、出错面大。**除非你明确要保留这两个提交的署名信息，否则走 A。**

### 方案 A：新 clone + 工作区覆盖（推荐）

**产出**：新仓库历史 = 远程完整历史 + 1 个恢复提交；代码 = 当前工作区；`.workbuddy/` 天然不入库。

```bash
# A1. 在项目外的临时位置 clone 远程（拿到干净的已推送历史）
cd /d/Family/DeepSeekHarness/plugins
$G clone https://github.com/kyorakuyk/dsh-wallpaper.git dsh-wallpaper-rebuild
cd dsh-wallpaper-rebuild

# A2. 切到与原分支同名的新分支，基于远程已推送的 tip
$G checkout -b codex/startup-bootstrap-retry origin/codex/startup-bootstrap-retry

# A3. 把工作区内容覆盖过来（排除依赖/产物/.git，以及 .workbuddy）
#     Windows: 用 robocopy，注意源是工作区备份，不是坏仓库
robocopy "D:\Family\DeepSeekHarness\plugins\_dsh-wallpaper-workspace-20260922" ^
        "D:\Family\DeepSeekHarness\plugins\dsh-wallpaper-rebuild" ^
        /E /XD ".git" "node_modules" "target" "artifacts" "dist" "dist-lite" ".workbuddy" ^
        /XF "_tmp_*" /R:1 /W:1

# A4. 检查差异，确认只有预期内容
$G status --short

# A5. 提交
$G add -A
$G commit -m "restore: local working tree after 2026-09-18 CapsuleBubble work

本地对象库在 2026-09-22 21:04 被中断的后台 git gc 破坏，未推送的
c89d65b / d1b90cd / c6d8e83 三个提交对象永久丢失。此提交以损坏前
的本地工作副本为准，重建其净效果。"
```

### 方案 B：抢救 `5f5e4de` / `3800601` 的提交历史（可选，步骤多）

只有在需要保留这两个提交的署名/提交信息时才走这条。

```bash
# B1. 在【备份副本】上重建 refs（绝不在坏仓库上做）
cd "D:\Family\DeepSeekHarness\plugins\.git-backup-dsh-wallpaper-20260922"
mkdir -p refs/heads refs/tags refs/remotes/origin/codex
printf 'f4907ee7070b856e1805467b897e07906f9985ba\n' > refs/heads/tmp-base
printf '5f5e4de64d697d540e287b47977d8c86b8677c97\n' > refs/heads/tmp-5f5e4de
printf '38006013b9955b28a28487e83564fa50cb45425d\n' > refs/heads/tmp-3800601

# B2. 导出资源包（只含 f4907ee 之后、3800601 之前的所有对象）
cd ..
$G --git-dir=".git-backup-dsh-wallpaper-20260922" bundle create rescue.bundle \
    f4907ee7070b856e1805467b897e07906f9985ba..tmp-3800601

# B3. 在新建的克隆里拉入
cd dsh-wallpaper-rebuild
$G fetch ../rescue.bundle tmp-3800601:rescue/prescrub

# B4. 剔除 .workbuddy 与 _tmp 垃圾（只在 f4907ee..rescue/prescrub 范围内）
$G filter-branch --force --index-filter \
    "git rm -r --cached --ignore-unmatch .workbuddy _tmp_43272_c96b5a9b57985f4f239f67dc1f3350c4" \
    --prune-empty -- f4907ee7070b856e1805467b897e07906f9985ba..rescue/prescrub
```

**B4 之后必须验证**（见第 3 节），确认 `.workbuddy` 在**所有**提交里都不存在：

```bash
$G log --all --oneline --diff-filter=A -- .workbuddy
# 期望：无任何输出
```

### 方案 C：原地修复（不推荐，仅在你不想换目录时用）

在坏仓库原地重建 refs，再 `git fetch` 远程补对象。**不推荐的原因**：坏 pack 仍留在 `.git/objects/pack` 里，后续 `fsck` / `gc` 会持续报错，且 pack 有截断，风险高于换一个干净目录。

---

## 3. 无论如何都要跑的验证

在恢复后的仓库里逐条确认：

```bash
# 1) 仓库可用、历史在线
$G log --oneline -5
$G branch -a

# 2) 对象库健康（这一步才能发现 pack 截断）
$G fsck --full
#    期望：无 error / missing / corrupt
#    ⚠️ 注意：verify-pack 报 ok 不代表健康，它在 pack 截断时仍会说 ok

# 3) 工作区干净
$G status

# 4) 跟踪文件数量对得上（损坏前是 353）
$G ls-files | wc -l

# 5) 隐私文件确实不在版本库
$G ls-files | grep -c workbuddy      # 期望 0
$G log --all --oneline -- .workbuddy # 期望无输出
$G ls-files | grep -E "_tmp_|_probe" # 期望无输出

# 6) 关键源码在位
test -f wallpaper/src-tauri/src/deepseek_web.rs && echo OK
test -f wallpaper/src-tauri/src/windows_integration.rs && echo OK
test -f bridge/package.json && echo OK
```

---

## 4. 收尾

```bash
# 1) 旧坏仓库改名保留（不要删）
#    D:\Family\DeepSeekHarness\plugins\dsh-wallpaper
#    → D:\Family\DeepSeekHarness\plugins\dsh-wallpaper-CORRUPT-20260922

# 2) 新仓库改成正式目录名
#    dsh-wallpaper-rebuild → dsh-wallpaper

# 3) 重装依赖并跑一遍 CI 等价检查
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
cargo test --manifest-path wallpaper/src-tauri/Cargo.toml --locked --all-targets
pnpm build && pnpm build:lite

# 4) 确认无误后再删备份
#    .git-backup-dsh-wallpaper-20260922
#    _dsh-wallpaper-workspace-20260922
#    dsh-wallpaper-CORRUPT-20260922
```

---

## 5. 顺带要做的一件事：`.gitignore`

本次事故暴露的隐私问题与恢复无关，但要在新仓库里一并堵上。`.gitignore` 末尾追加：

```gitignore
# Agent workspace (memory / scratch) —— 工作日志含私人分析与跨项目引用，绝不入库
.workbuddy/
.hermes/

# Ad-hoc agent scratch output at repo root
/_tmp_*
/_probe*
/_loc.txt
/_test*.txt
/_tsc.txt
```

**注意**：`.gitignore` 对**已跟踪**文件无效。如果方案 B 的 `filter-branch` 没跑干净，必须用 `git rm --cached` 显式移除。

---

## 6. 最后一条注意事项

`git push` 之前，确认第 3 节第 5 条的输出全部符合预期。

原因是：`kyorakuyk/dsh-wallpaper` 是 **PUBLIC** 仓库。一旦把含 `.workbuddy/memory/2026-09-15.md` 的提交推上去，那份 1153 行的私人笔记（内含面试策略、slimeMold 项目分析与 `D:\code` 路径引用）就会进入公开历史 —— 之后要清理就得 force push 公开仓库。
