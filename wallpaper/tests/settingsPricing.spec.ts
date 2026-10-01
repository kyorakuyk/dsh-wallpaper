import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { t } from '../src/i18n/index.ts'
import { MAX_PRICE_PER_MILLION, normalizeSettings, normalizedPrice } from '../src/settings/store.ts'
import { PriceInput, SettingsPanel } from '../src/settings/SettingsPanel.tsx'

describe('API pricing setting normalization', () => {
  it('keeps valid zero pricing but rejects malformed or unsafe rates', () => {
    expect(normalizedPrice(0)).toBe(0)
    expect(normalizedPrice(MAX_PRICE_PER_MILLION)).toBe(MAX_PRICE_PER_MILLION)
    expect(normalizedPrice(MAX_PRICE_PER_MILLION + 0.01)).toBeUndefined()
    expect(normalizedPrice(-1)).toBeUndefined()
    expect(normalizedPrice(Infinity)).toBeUndefined()
    expect(normalizedPrice('1')).toBeUndefined()
  })

  it('exposes the native price ceiling in the settings input', () => {
    // 这条与冻结不冲突，所以原样留着：`PriceInput` 组件本身一行没删，上限仍然跟着原生常量走。
    // 冻的只是它在设置页上的**两处挂载点**（见下面「价格输入冻结之后」）。
    const html = renderToStaticMarkup(createElement(PriceInput, {
      label: '输入价格',
      value: undefined,
      onChange: () => undefined,
    }))

    expect(html).toContain(`max="${MAX_PRICE_PER_MILLION}"`)
  })
})

/**
 * 价格输入冻结之后（用户要求界面上不再出现"计价"）。
 *
 * 上一批冻的是**读数**（顶栏用量串、每条消息下面那行费用，见 `ConversationBubble.tsx` 的几处
 * FREEZE）；这一批冻的是**喂它的输入**：`SettingsPanel.tsx` 里那两行 `Field`（输入价格 /
 * 输出价格）连同它们的 `detail` 一起注销。理由正是"那句说明现在是假的"—— 它承诺的
 * "本轮和会话估算费用"已经不在界面上了，而两个改了也看不见任何变化的输入框比它们不在更坏。
 *
 * 两条主线，与 `ConversationBubble.spec.tsx` 的「用量与费用读数冻结之后」同源：
 * 1. **渲染结果里不再出现**：这里渲染的是**真的**设置面板（不是手抄一份期望），并且先用同一页
 *    上还活着的东西做正面锚点，免得断言因为"整页没渲染"而假绿；
 * 2. **源码里仍以注释存在、且写明了恢复办法**：否则将来谁都可以把它悄悄删掉。
 *
 * 数据层不动也是契约的一部分：`store.ts` 的字段与归一化、`PriceInput` 组件、上限常量照旧。
 */
describe('价格输入冻结之后', () => {
  async function panelSource(): Promise<string> {
    return (await readFile(resolve(dirname(fileURLToPath(import.meta.url)), '../src/settings/SettingsPanel.tsx'), 'utf8'))
      .replace(/\r\n?/g, '\n')
  }

  /** 剥掉注释之后还剩什么 —— 与 `noHardcodedCopy.spec.ts` 同一个剥法（注释里的中文是允许的）。 */
  function withoutComments(source: string): string {
    return source.replace(/\/\*[\s\S]*?\*\//g, '')
      .split('\n')
      .map((line) => {
        const at = line.search(/(^|\s)\/\//)
        return at >= 0 ? line.slice(0, at) : line
      })
      .join('\n')
  }

  /**
   * 一份能真的渲染出「连接」页的设置面板。
   *
   * 价格**故意配好**（0 与 4：零是有效配置，见上面那条归一化测试）—— 这样"渲染里没有输入框"
   * 就不是因为"没什么可显示"，而是因为它确实被冻住了。语言是当前语言（默认中文），面板与这里的
   * `t()` 取的是同一份，所以两侧不会错位。
   */
  function panelProps() {
    const settings = normalizeSettings({})
    return {
      settings: {
        ...settings,
        deepseekApi: { ...settings.deepseekApi, priceInputPerMillion: 0, priceOutputPerMillion: 4 },
      },
      page: 'connections' as const,
      onPageChange: () => undefined,
      harnessStatus: 'offline' as const,
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
      autostart: { enabled: false, source: 'none' as const, reason: null },
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
      reachAction: 'browser' as const,
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
    }
  }

  it('renders the connections page without either price input, even with both prices configured', () => {
    const html = renderToStaticMarkup(createElement(SettingsPanel, panelProps()))

    // 正面锚点：这一页真的渲染出来了，而且就是「连接」页 —— 否则下面每一句 not.toContain 都会假绿。
    expect(html).toContain(t('nav.connections.label'))
    expect(html).toContain(t('settings.connections.api.title'))
    expect(html).toContain(t('settings.connections.api.key.title'))
    expect(html).toContain(t('settings.connections.api.model.title'))
    expect(html).toContain('deepseek-chat')
    // 价格两项与本页其他字段的区别就在这里：配置了也不出现（价格配的是 0 与 4）。
    expect(html).not.toContain('price-input')
    expect(html).not.toContain(t('settings.connections.api.price-input.title'))
    expect(html).not.toContain(t('settings.connections.api.price-output.title'))
    expect(html).not.toContain(t('settings.connections.api.price-input.label'))
    expect(html).not.toContain(t('settings.connections.api.price-output.label'))
    // 两句说明整句都不在了 —— 尤其「输入价格」那句承诺"才会显示本轮和会话估算费用"的假话。
    expect(html).not.toContain(t('settings.connections.api.price-input.detail'))
    expect(html).not.toContain(t('settings.connections.api.price-output.detail'))
    expect(html).not.toContain('人民币')
    expect(html).not.toContain('估算费用')
  })

  it('keeps both frozen rows in the file, so reviving them needs no git archaeology', async () => {
    const source = await panelSource()
    // 两行都还在（在注释里），**逐字**保留：字段、`onChange`、连那句假话 `detail=` 的原样都在，
    // 恢复时是取消注释而不是重写。
    expect(source).toContain("{/* <Field title={t('settings.connections.api.price-input.title')} detail={t('settings.connections.api.price-input.detail')}><PriceInput")
    expect(source).toContain("{/* <Field title={t('settings.connections.api.price-output.title')} detail={t('settings.connections.api.price-output.detail')}><PriceInput")
    // 说明写齐三件事：为什么关、关掉之后什么变了、怎么恢复。
    expect(source).toContain('FREEZE')
    expect(source).toContain('为什么关')
    expect(source).toContain('关掉之后')
    expect(source).toContain('怎么恢复')
  })

  it('leaves nothing alive that could mount a price input or promise an estimate', async () => {
    const alive = withoutComments(await panelSource())
    for (const symbol of [
      'settings.connections.api.price-input.title',
      'settings.connections.api.price-input.detail',
      'settings.connections.api.price-input.label',
      'settings.connections.api.price-output.title',
      'settings.connections.api.price-output.detail',
      'settings.connections.api.price-output.label',
    ]) {
      expect(alive, `${symbol} 仍然活在代码里`).not.toContain(symbol)
    }
    // 但冻的是**价格输入**，不是整张 API 卡片，也不是 `PriceInput` 组件本身（数据层留着）：
    // 否则上面那几条会自动变成假绿。
    expect(alive).toContain('settings.connections.api.key.title')
    expect(alive).toContain('settings.connections.api.model.title')
    expect(alive).toContain('settings.connections.api.price.placeholder')
    expect(alive).toContain('export function PriceInput(')
    expect(alive).toContain('normalizedPrice')
  })
})
