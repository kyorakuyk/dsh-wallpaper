# dsh-wallpaper 主题包与独立素材系统计划

## 1. 用户模型与术语

目标是让普通用户无需理解目录结构、manifest 或哈希，即可完成导入、分类、切换和分享。

正式术语：

- **官方主题**：随应用安装的只读默认外观。
- **主题包（Theme Package）**：成套启用的外观包；包内素材不暴露到全局单项选择菜单。
- **独立素材（Loose Asset）**：单独导入的背景、立绘、字体等资源。
- **组件素材库（Asset Library）**：已完成分类、可用于单项替换的独立素材列表。
- **待分类区（Inbox）**：已经导入但尚未指定用途的素材。
- **局部覆盖（Override）**：在当前主题上临时替换一个组件。
- **内容哈希（Content Hash）**：底层使用 SHA-256 去重和验证，不在普通操作界面突出显示。

外观解析顺序：

```text
官方主题基线
→ 当前用户主题包
→ 当前主题上的局部覆盖
```

规则：

- 官方主题与用户主题出现在同一个主题菜单中。
- 官方主题固定置顶、只读、不可删除，但物理存储在程序资源目录。
- 用户主题存储在应用数据目录。
- 主题包内部资源为私有资源，不进入独立素材库。
- 独立素材允许覆盖主题中的单个组件。
- 切换任何主题时清空全部局部覆盖。
- 未切换主题时，局部覆盖跨应用重启保留。
- “恢复主题默认”只清除当前局部覆盖。

## 2. 可定制组件

首版公开以下槽位：

```ts
type AppearanceSlot =
  | 'desktop.background'
  | 'lockscreen.image'
  | 'wake.sequence'
  | 'persona.deepseek.flash'
  | 'persona.deepseek.pro'
  | 'persona.harness.flash'
  | 'persona.harness.pro'
  | 'chat.skin'
  | 'ui.font'
```

- 单张图片可分配为背景、锁屏或一个以上立绘槽位。
- 立绘必须由用户手动勾选适用形态，未分类前不得出现在任何立绘菜单。
- 图片序列可分配为苏醒动画，并允许确认顺序、单帧时长和淡入时间。
- 字体支持 TTF、OTF、WOFF2。
- 气泡皮肤由声明式参数和可选纹理组成，不接受任意 CSS 或脚本。
- 首版只支持手动选择，不做随机、定时或顺序轮换。

## 3. 导入、分类与存储

### 3.1 支持的入口

用户可以：

- 将文件或文件夹拖入外观管理页。
- 通过托盘选择“导入文件或文件夹”。
- 导入 ZIP 或 `.dshwallpaper`。

导入识别顺序：

1. 根目录包含 `theme.json`：按主题包处理。
2. 根目录包含 `component.json`：按组件包处理。
3. 没有声明文件的文件夹：整批导入待分类区。
4. 单个图片或字体文件：导入待分类区。

系统可以根据扩展名、尺寸、透明通道和文件名提出分类建议，但必须由用户确认。在确认前：

- 文件已经复制到应用素材库。
- 文件已完成哈希去重和安全检查。
- 不自动替换当前桌面。
- 不出现在正式组件菜单。
- 可预览、删除或批量分类。

### 3.2 分类向导

向导流程：

1. 显示素材缩略图、尺寸、格式和透明通道信息。
2. 给出“可能是立绘/背景/锁屏/帧序列/字体”的非强制建议。
3. 用户勾选一个或多个兼容槽位。
4. 立绘允许多选四种形态。
5. 帧序列允许拖拽排序并预览播放。
6. 用户确认后，素材才加入对应组件素材库。

不兼容的选择必须在 UI 中禁用，例如字体不能分配为背景，单张图片不能直接分配为帧序列。

### 3.3 内容寻址存储

```text
%LOCALAPPDATA%\dsh-wallpaper\
  catalog.db
  settings.json
  library\
    objects\sha256\<prefix>\<hash>.<ext>
    themes\<theme-id>\<version>\theme.json
    inbox\
    previews\
    staging\
  exports\
  diagnostics\
```

- 所有用户导入内容复制到应用数据目录，不长期引用源路径。
- 文件内容以 SHA-256 标识；同一文件重复导入只保存一个对象。
- SQLite 保存资源元数据、分类、主题引用、当前主题和局部覆盖。
- 导入先进入 `staging`，全部校验通过后事务提交。
- 删除操作先删除引用；无主题、分类或覆盖引用的对象才允许垃圾回收。
- 官方主题不复制到用户目录，但通过统一查询接口参与主题列表。

建议数据实体：

```ts
interface AssetRecord {
  id: string
  sha256: string
  mediaType: 'image' | 'font' | 'sequence' | 'skin'
  originalName: string
  objectPath: string
  width?: number
  height?: number
  hasAlpha?: boolean
  status: 'inbox' | 'classified' | 'corrupt'
  slots: AppearanceSlot[]
  createdAt: number
}

interface ThemeRecord {
  id: string
  version: string
  source: 'official' | 'user'
  manifestPath: string
  readonly: boolean
  installedAt?: number
}
```

## 4. 主题包规范

主题包是纯声明式、自包含或显式继承的压缩包。

```text
my-theme.dshwallpaper
├─ theme.json
├─ preview.webp
├─ scenes/
├─ personas/
├─ ui/
└─ text/
```

建议 manifest：

```ts
interface ThemeManifest {
  schemaVersion: 1
  kind: 'theme'
  id: string
  version: string
  name: string
  author?: string
  description?: string
  preview?: string
  compatibility: {
    minAppVersion: string
  }
  baseline: {
    id: string
    version: string
  }
  components: Partial<Record<AppearanceSlot, ThemeComponent>>
  ui?: {
    tokens?: string
    text?: string
    layout?: string
  }
  files: Array<{
    path: string
    sha256: string
    size: number
  }>
}
```

### 4.1 缺省与继承

- 主题不必提供所有核心组件。
- 缺失组件必须继承指定的官方主题版本，不能依赖“当前最新官方主题”。
- 导入预览必须列出所有缺省项，例如“未提供锁屏图，将继承官方睡眠图”。
- 用户确认后才能安装或启用。
- 导入时把实际使用的官方依赖按内容哈希固定到共享依赖缓存，确保应用升级后旧主题外观不变。
- 可在主题详情中提供“迁移到新版官方基线”，但不得自动迁移。

### 4.2 私有资源

- 主题包中的组件只能由该主题使用。
- 包内背景、立绘和字体不得出现在独立素材切换菜单。
- 主题详情允许查看组件来源、大小和继承状态，但不提供“提取到素材库”。
- 用户可以使用真正的独立素材覆盖当前主题。

### 4.3 兼容与失败处理

- `schemaVersion` 不支持：拒绝导入并显示所需应用版本。
- 文件哈希不匹配：拒绝启用。
- 某个已安装对象丢失或损坏：该组件回退到锁定的官方基线，并报告可恢复错误。
- 主题更新作为新版本并列安装；旧版本在仍被使用时不能被覆盖删除。

## 5. 主题导出

首版支持“把当前桌面导出为主题包”。

导出规则：

- 当前局部覆盖转为新主题的正式组件。
- 所有继承项复制进导出包。
- 输出包完全自包含，接收方不需要相同的官方基线版本。
- 自动生成 SHA-256 文件清单、manifest 和预览图。
- 导出前要求填写名称、版本、作者和可选说明。
- UI 提示用户确认自己拥有素材分享权，但应用不执行版权判断。
- 导出包先写入临时位置，完整校验通过后再移动到目标路径。

验收标准：在一个全新的用户数据目录中导入导出包，背景、四形态立绘、锁屏、苏醒动画、气泡皮肤和字体应与导出时一致。

## 6. UI 与托盘流程

### 6.1 外观管理页

右侧设置抽屉中的“外观”页面依次显示：

1. 主题选择：官方主题固定置顶，用户主题在其后。
2. 当前主题预览、版本、继承项和局部覆盖状态。
3. 背景、当前形态立绘、锁屏、苏醒、气泡皮肤和字体选择。
4. “恢复主题默认”。
5. 组件素材库。
6. 带数量角标的待分类区。
7. 导入文件/文件夹和导出当前主题。
8. 删除、重新分类、查看来源和完整性检查。

主题包内部素材只在主题详情中显示，不出现在第 3、5 项列表。

### 6.2 托盘菜单

```text
显示/隐藏交互界面
主题
  官方主题
  用户主题……
背景
  使用主题默认
  已分类的独立背景……
当前形态立绘
  使用主题默认
  当前槽位的独立立绘……
显示/隐藏聊天气泡
导入文件或文件夹……
打开外观管理
设置
退出
```

- 托盘选择独立素材会创建当前主题的局部覆盖。
- 切换主题后覆盖清空，菜单立即同步。
- 托盘只承担常用快速操作；分类、删除、批量管理和导出在外观管理页完成。
- 设置与素材管理作为应用内 Drawer/Popover 展示，不创建普通顶层窗口。

## 7. 公开接口

Rust 向前端提供类型化外观 API：

```ts
interface AppearanceApi {
  getAppearanceState(): Promise<AppearanceState>
  listThemes(): Promise<ThemeSummary[]>
  activateTheme(themeId: string, version?: string): Promise<void>
  importPaths(paths: string[]): Promise<ImportBatch>
  classifyAssets(request: ClassificationRequest): Promise<void>
  listAssets(slot?: AppearanceSlot): Promise<AssetSummary[]>
  setOverride(slot: AppearanceSlot, assetId: string): Promise<void>
  clearOverride(slot?: AppearanceSlot): Promise<void>
  deleteAsset(assetId: string): Promise<void>
  exportCurrentTheme(meta: ExportThemeMetadata): Promise<string>
  validateTheme(path: string): Promise<ThemeValidationReport>
}
```

统一事件：

```ts
type AppearanceEvent =
  | { type: 'appearance-changed'; state: AppearanceState }
  | { type: 'library-changed' }
  | { type: 'import-progress'; batchId: string; completed: number; total: number }
  | { type: 'import-complete'; batchId: string }
  | { type: 'asset-corrupt'; assetId: string; fallbackApplied: boolean }
```

所有改变主题、分类或覆盖关系的操作必须由 Rust 完成并以事务方式更新数据库。前端不得直接写素材目录或自行维护另一份外观状态。

## 8. 安全限制

- 拒绝 ZIP 路径穿越、绝对路径、符号链接和设备路径。
- 限制单文件大小、文件总数、压缩包大小与解压倍率。
- 禁止 EXE、DLL、脚本和其他可执行内容。
- SVG 必须移除或拒绝脚本、事件属性、外部 URL 和外部字体。
- 字体经过格式验证后才允许注册。
- 主题不能访问网络、Cookie、API Key、DSH Token 或任意文件系统。
- 预览解码失败不得影响主进程，失败素材留在待分类区并标记错误。
- 导入、校验和缩略图生成不能记录图片正文或敏感路径到普通日志。

## 9. 测试与验收

自动化测试至少覆盖：

- 官方主题只读、置顶，并与用户目录隔离。
- 无 manifest 文件夹进入待分类区。
- 未分类素材不进入组件菜单。
- 单一素材可分配到多个立绘槽位。
- 相同内容重复导入只生成一个对象。
- 主题内部资源不泄露到独立素材库。
- 切换主题清除局部覆盖。
- 未切换主题时覆盖跨重启恢复。
- 缺省项在导入确认页完整展示。
- 官方主题升级后旧主题视觉不变。
- 导出包在全新环境中可独立还原。
- 路径穿越、脚本、损坏哈希、超限文件和压缩炸弹被拒绝。
- 对象丢失时回退到锁定官方基线。
- 删除仍被主题或覆盖引用的素材时给出明确阻止原因。

用户体验验收：

- 普通用户不用编辑 JSON 即可导入一张立绘并分配给一个或多个形态。
- 导入普通文件夹后，可以批量预览、分类和启用。
- 用户可以从托盘快速切换主题、背景和当前形态立绘。
- 用户能够一键恢复当前主题默认状态。
- 主题包的内部素材不会混入独立素材菜单。
- 当前搭配可以导出，并在另一台机器获得一致结果。

## 10. 建议交付拆分

可分别交付并通过接口集成：

1. `catalog.db` schema、迁移和 Rust repository。
2. 内容哈希对象库、staging 事务与垃圾回收。
3. ZIP/文件夹主题验证器与安全检查。
4. 待分类区和分类向导 UI。
5. 主题选择、局部覆盖和托盘菜单。
6. 自包含主题导出器。
7. 单元测试、损坏恢复和全新目录导入 E2E。

## 11. 里桌面组件扩展边界

主题包负责声明视觉素材与设计 token；可执行桌面组件必须作为独立 Widget 插件安装，不能借主题包夹带脚本。建议公开接口：

```ts
interface DesktopWidgetPlugin {
  manifest: WidgetManifest
  create(context: WidgetContext): DesktopWidget
}

interface WidgetContext {
  workspace: 'front' | 'inner'
  geometry: DesktopGeometry
  theme: Readonly<ThemeTokens>
  storage: WidgetStorage
  events: WidgetEventBus
}
```

`WidgetManifest` 至少声明插件 ID、版本、宿主兼容范围、默认/最小/最大尺寸、默认锚点、设置 schema、权限和支持的工作区。官方默认组件与用户插件沿用“官方基线不与用户包并列”的原则，但在组件管理页明确展示来源。

权限默认全部关闭；网络、文件选择、通知和 DSH 能力必须逐项声明并由宿主代理。组件不得直接读取 API Key、Cookie、网页登录态或 bridge token。布局和启用状态由宿主版本化保存，插件卸载后保留设置需要用户明确选择。

任何子任务都不得绕过统一 `AppearanceApi` 直接修改运行中的 React 状态或用户素材目录。
