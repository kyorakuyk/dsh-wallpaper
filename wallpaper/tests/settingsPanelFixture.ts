import { normalizeSettings } from '../src/settings/store.ts'
import type { SettingsPanelProps } from '../src/settings/SettingsPanel.tsx'

/**
 * 一份能真的渲染出设置面板的 props（默认停在「连接」页）。
 *
 * 默认值取"什么都还没读到"的最小集：没有执行主体目录、没有扫描结果、没有连接、没有外观素材 ——
 * 这一份能渲染出真实的页面结构，而不是一个空壳。
 *
 * 从 `settingsPricing.spec.ts` 里搬出来的：那个文件原来自己攒了这么一份，而「无控件 Field 的
 * 布局」那组回归测试同样要渲染**真的**面板 —— `Field` 没有单独导出，而手抄一份期望的 DOM 正是
 * 那个文件明说过不想做的事（"这里渲染的是真的设置面板，不是手抄一份期望"）。同一份 props 只留
 * 一处，免得两边慢慢长歪。
 *
 * 要别的状态就覆盖那一项：`settingsPanelProps({ page: 'system', autostart: { ... } })`。
 */
export function settingsPanelProps(overrides: Partial<SettingsPanelProps> = {}): SettingsPanelProps {
  return {
    settings: normalizeSettings({}),
    page: 'connections',
    onPageChange: () => undefined,
    harnessStatus: 'offline',
    onChange: () => undefined,
    onRequestDeepSeekLogin: () => undefined,
    apiKeyDraft: '',
    onApiKeyDraftChange: () => undefined,
    apiKeyBusy: false,
    onTestApiKey: () => undefined,
    onRefreshApiModels: () => undefined,
    apiModelCatalog: [{ id: 'deepseek-chat', name: 'DeepSeek Chat' }],
    onSelectBackend: () => undefined,
    onClose: () => undefined,
    interactionEnabled: false,
    onSetInteractionEnabled: () => undefined,
    harnessTargets: [],
    onSelectSubject: () => undefined,
    autostart: { enabled: false, source: 'none', reason: null },
    onScanDsh: () => undefined,
    dshScanBusy: false,
    endpointScan: [],
    endpointScanBusy: false,
    endpointScanDone: false,
    onScanEndpoints: () => undefined,
    onOpenClient: () => undefined,
    onOpenTui: () => undefined,
    onSelectWindow: () => undefined,
    tuiAvailable: false,
    reachAction: 'browser',
    openBusy: false,
    managedDsh: { instances: [], managed: false, running: false },
    managedDshBusy: false,
    onRefreshManagedDsh: () => undefined,
    onStopAllManagedDsh: () => undefined,
    onRefreshDeepSeekWebAdapterConfig: () => undefined,
    onOpenDeepSeekWebAdapterConfig: () => undefined,
    onResetDeepSeekWebAdapterConfig: () => undefined,
    appearanceAssets: [],
    appearanceOverrides: {},
    appearanceBusy: false,
    onImportAppearance: () => undefined,
    onClassifyAppearance: () => undefined,
    onSelectAppearance: () => undefined,
    onClearAppearance: () => undefined,
    autostartBusy: false,
    desktopDisplays: [],
    onRefreshDesktopDisplays: () => undefined,
    apiHistoryBusy: false,
    onRefreshApiHistory: () => undefined,
    onDeleteApiConversation: () => undefined,
    onClearApiHistory: () => undefined,
    // 系统页「更新」那张卡片：这几项都是**必填**的。原来那份 props 里一个都没有 ——
    // 测试不被 typecheck（`tsconfig.json` 的 include 只有 `src`），所以一直没被发现；
    // 补上它们之后这份 fixture 才真的等于"面板能渲染"。
    updateBusy: false,
    onCheckForUpdates: () => undefined,
    onDownloadUpdate: () => undefined,
    onInstallUpdate: () => undefined,
    onOpenUpdatePage: () => undefined,
    onDismissUpdate: () => undefined,
    ...overrides,
  }
}
