# 桥插件与 DSH 宿主的版本兼容（0.2.0-rc.1 实测记录）

记录时间：2026-09-29。被测环境：DSH 桌面壳与 CLI 均在 `0.2.0-rc.1`，桥插件 `dsh-wallpaper-bridge` 0.1.2 到 0.1.3，
Windows 11 x64。目的有两个：把这次"连不上"的因果关系写清楚，以及给出**下次宿主升级时该怎么处置**的流程。

---

## 一、症状与判词

现象：从壁纸拉起官方桌面壳或 CLI 都"启动成功但连接失败"，界面上 Bridge 显示未连接。

宿主自己给出了判词（stderr 原话，CLI 与壳同样）：

```
dsh: skipping profile bundle "dsh-wallpaper-bridge": Error: Plugin
dsh-wallpaper-bridge@0.1.2 is incompatible with dsh 0.2.0-rc.1: peerDependencies
{"@deepseek-ai/dsh-agent":"^0.1.0-rc.5","@deepseek-ai/dsh-host-webserver":"^0.1.0-rc.5",
 "@deepseek-ai/dsh-llm":"^0.1.0-rc.5","@deepseek-ai/dsh-session":"^0.1.0-rc.5",
 "@deepseek-ai/dsh-user-approval":"^0.1.0-rc.5"}.
…
To accept this risk explicitly, grant the exact-version exemption for
dsh-wallpaper-bridge@0.1.2 on dsh 0.2.0-rc.1 with `dsh plugin allow-version` …
```

即：**宿主拒绝加载 peer 范围不覆盖自己版本的插件**，插件被整包跳过，路由从未注册，壁纸自然连不上。

### 一条把排查带偏的线索（留档）

插件被跳过之后，`/api/wallpaper/v1/status` 落到了**宿主自己的鉴权墙**上，返回 `401 unauthorized` —— 与
任意无意义路径（`/api/nonsense/xyz`）返回的一模一样。因此"401"看着像"插件在但没鉴权"，实际是"插件根本不在"。
判别方法见第四节：**桥的路由是绕过那道墙的**，插件真挂上时 `/status` 会直接给 200。

---

## 二、机制

- 判定读的是**部署副本的 `package.json`** 里的 `peerDependencies`（宿主原文把五个包和范围原样引出来了）。
- 插件**运行时代码里内联的**清单是另一份：`/status` 报的 `bridgeVersion` / `authoredAgainst` 来自构建产物
  （`lib/`），它与部署副本的 `package.json` 可以不一致 —— 实测就出现过"宿主按 0.1.3 放行、`/status` 仍报 0.1.2"
  的组合（当时部署副本是 0.1.3、但 `lib/` 还是旧构建）。
- 结论：**改完范围必须重新构建并重新部署**，否则会出现"宿主放行但跑的是旧代码"这种最难查的状态。

---

## 三、修法与范围覆盖

`bridge/package.json` 五条宿主 peer 改为 `^0.1.0-rc.5 || ^0.2.0-rc.1`，插件版本提到 `0.1.3`。
用两套 semver 实现（npm 自带 7.7.4 与工作区锁里已有的 6.3.1）核过，结论一致：

| 版本 | `^0.1.0-rc.5 \|\| ^0.2.0-rc.1` |
| --- | --- |
| 0.1.0-rc.5 / 0.1.5 | 接受 |
| 0.1.7-rc.2 | 拒绝（见下方注意） |
| 0.2.0-rc.1 / **0.2.0-rc.2** / 0.2.0 / 0.2.1 / 0.2.9 | 接受 |
| 0.3.0-rc.1 / 0.3.0 | 拒绝 |

**注意（实测反例）**：`0.1.7-rc.2` 在严格 semver 下不满足 `^0.1.0-rc.5`（预发布版只匹配同一元组上的比较子），
可它当时**确实被宿主加载了** —— 说明宿主自己那道检查比严格 semver 宽松。因此：

- 同一 minor 元组的后续预发布版（0.2.0-rc.2 等）**不需要改代码**，这是 semver 的既有语义；
- 但"新版本一定能跑"不能用 semver 推断，**只能靠 smoke 测试证**（第五节）；
- 换 minor 元组（0.3.x）**必须**是一次有人确认的适配，不该靠范围放行；
- 潜伏的一条：声明里对 0.1.x 只精确覆盖 `0.1.0-rc.5` 这一代预发布版，`0.1.7-rc.2` 这类**预发布版在严格
  semver 下会被拒**（现在靠宿主宽松才通过）。哪天宿主改成严格判定，跑 0.1.x 预发布版的机器会突然被拒；
  到那时要么把那些版本显式列进范围，要么直接把 0.1.x 这条线退休。

守卫测试：`bridge/tests/hostCompatibility.spec.ts`（断言上面这张表里的"必须接受/必须拒绝"，防止有人顺手收窄范围）。

---

## 四、验证方法（只读）

```
GET http://127.0.0.1:3080/api/wallpaper/v1/status
```

插件挂上时返回 200，形如：

```json
{"bridgeVersion":"0.1.3","protocolVersion":1,"dsh":"online",
 "authoredAgainst":"^0.1.0-rc.5 || ^0.2.0-rc.1","state":"bridge-ready","reasonCode":"ready",
 "capabilities":["status","control","sessions","history","sse","cancel","approval-handoff","resume"],
 "authentication":"ready"}
```

判别要点：同一次测试里再探一个无意义路径（例如 `/api/nonsense/xyz`），它应当仍是 `401`。
两者一起出现才说明"桥的路由绕过了宿主的鉴权墙、插件确实挂上了"；若两条都是 401，就是插件没加载。

补充信号（各自独立，别只看一个）：PID 由端口反查、`~/.dsh/profiles/<p>/node_modules/...` 的时间戳与版本。
不要用"桥令牌文件的 mtime"当信号 —— 桥复用既有令牌、启动时并不重写它（实测 08-24 的令牌配 09-29 的进程）。

---

## 五、真宿主 smoke 测试（权威判据）

`bridge/tests/realDshSmoke.spec.ts` 会用**真 DSH CLI** 起一个**临时 profile**（`DSH_HOME` 指向临时目录，
不读不写用户的会话/凭据/令牌），把本仓库的桥构建以 junction 挂进去，然后逐条断言"声明的能力都有可达路由"。
它是 opt-in 的，靠两个环境变量指认被测宿主：

```
$env:DSH_WALLPAPER_SMOKE_DSH_ROOT = "<DSH 检出目录>"
$env:DSH_WALLPAPER_SMOKE_PROFILE = "$env:USERPROFILE\.dsh\profiles\web"
pnpm -C bridge test
```

**宿主升级的标准动作**：升级前先跑它，跑通再升；升完再跑一次。这比任何 semver 推断都可靠。

---

## 六、两个部署坑（都会让"改了却没生效"）

1. **pnpm 对 `file:` 依赖用硬链接**。插件的 `lib/` 由 tsdown 生成，而它是"删旧建新"：重建后部署副本里的硬链接
   仍指向**旧 inode**。实测 `install`、`update`、`install --force` 三种写法**都不刷新**部署副本；
   只有先 `remove` 再 `add` 才会重建链接：

   ```
   dsh plugin --profile web remove dsh-wallpaper-bridge
   dsh plugin --profile web add dsh-wallpaper-bridge@file:D:/Family/DeepSeekHarness/plugins/dsh-wallpaper/bridge
   ```

2. **`desktop` profile 由 Electron 壳独占管理**。CLI 对它做任何写操作（包括 `allow-version`）都会被拒：
   `profile "desktop" is managed exclusively by the Electron application`。
   所以官方桌面壳那条路只能在**壳内部**（它自己的插件管理器）刷新或授权。

---

## 七、临时逃生口：精确版本豁免

宿主给的官方出路，范围是"**这个插件版本 + 这个宿主版本**"这一对：

```
dsh plugin allow-version dsh-wallpaper-bridge@<插件版本> --dsh-version <宿主版本> --accept-risk --profile <profile>
```

- 记录位置：`~/.dsh/profiles/<profile>/compatibility.json`（撤销就是删掉对应条目）；
- 宿主会明确警告 "may cause crashes or data loss"，所以它**只适合临时过渡**：同一 minor 的新 rc 又要再授一次；
- 我们的立场：优先改 peer 范围并重建部署；豁免只用于"等上游确认期间先能用"。

---

## 八、这次改动清单（供回查）

| 文件 | 改动 |
| --- | --- |
| `bridge/package.json` | 五条宿主 peer 放宽到 `^0.1.0-rc.5 \|\| ^0.2.0-rc.1`；版本 0.1.2 到 0.1.3；新增 devDependency `semver@^6.3.1`（守卫测试用，复用工作区锁里已有的版本） |
| `bridge/tests/hostCompatibility.spec.ts` | 新增：把"必须接受/必须拒绝"的范围表固化 |
| 本文件 | 这次实测的因果、验证方法、部署坑与升级流程 |

未变更、但需要知道的一点：`bridge/package.json` 的 **devDependencies 仍指向 `^0.1.0-rc.5`**，
即本地**类型层面**的开发环境还是 0.1.x。这不影响运行时（运行时的形状断言是在真宿主上跑过的，桥在
0.2.0-rc.1 上 `bridge-ready`），但意味着"用类型检查发现宿主形状变化"这条路暂时不生效；
要获得那份收益，需要一次单独决定的 devDependencies 升级（会动工作区锁文件）。
