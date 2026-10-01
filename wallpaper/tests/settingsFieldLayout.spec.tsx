import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'

import { setLanguage } from '../src/i18n/index.ts'
import { en } from '../src/i18n/en.ts'
import { zh } from '../src/i18n/zh.ts'
import type { AutostartStatus } from '../src/native/runtime.ts'
import { normalizeSettings } from '../src/settings/store.ts'
import { SettingsPanel, type SettingsPanelProps } from '../src/settings/SettingsPanel.tsx'
import { settingsPanelProps } from './settingsPanelFixture.ts'

/**
 * 「没有控件的 Field」那一行的布局。
 *
 * 用户真机上截图报的：设置中心 →「连接」页里「壁纸自启未生效」那条警告的标题与说明被挤成
 * **一词一行**，右边还横着一条**不换行**的长句子、一直溢出到卡片外。两件事同一个原因：
 * 那句话被当成 `children` 传进了 `Field`，于是落进**控件列**（`.settings-field__control`），
 * 而控件列是 `flex: 0 0 auto` —— 按 max-content 定宽、不收缩、不折行；唯一能收缩的文案列
 * 于是被挤到只剩一个词宽。
 *
 * 这里钉的是**结果**，不是当时的写法：
 * 1. 那句话在文案列里（`__note`），这一行根本没有控件列；
 * 2. 全站每一行的控件列要么不存在、要么第一个孩子是**元素**（裸文本就是这次事故的形态）；
 * 3. CSS 里两列都能收缩（`flex: 0 0 auto` 一旦回来，这一条立刻红）；
 * 4. 文案短到一行能读完，而且指的那一页真的放着那个开关。
 *
 * 为什么用 `renderToStaticMarkup` 而不是量宽度：jsdom/`renderToStaticMarkup` 都没有排版引擎，
 * 量不出"一词一行"。所以这里量的是**结构**（谁在哪个列里）+ 源码侧的契约（CSS 规则本身），
 * 与 `chatGutter.spec.ts`、`ballAppearance.spec.tsx` 对 CSS 的做法一致。真机观感仍然要靠人看。
 */
const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

const FIELD = '<div class="settings-field">'
const COPY = '<div class="settings-field__copy">'
const CONTROL = '<div class="settings-field__control">'
const CLOSE = '</div>'
const NOTE = 'settings-field__note'

/**
 * 从某个 `div` 的开放标签开始，取到与它配对的那个 `</div>` 为止。
 *
 * 数配对的 `</div>`（控件里还会有别的 div：下拉、动作条），而不是取"第一个 `</div>`"：
 * 后者会把一行截短，也会让"这一行到哪里结束"变成猜测 —— 之前按下一个行的开放标签切分时，
 * 上一行的块会一直伸到下一张卡片的标题里，于是"工作区"这两个字会先在**上一行**被找到。
 */
function elementAt(html: string, start: number): string {
  let depth = 0
  let at = start
  for (;;) {
    const open = html.indexOf('<div', at)
    const close = html.indexOf(CLOSE, at)
    if (close < 0) return html.slice(start)
    if (open >= 0 && open < close) {
      depth += 1
      at = open + '<div'.length
      continue
    }
    depth -= 1
    at = close + CLOSE.length
    if (depth === 0) return html.slice(start, at)
  }
}

/** 渲染出来的每一行 —— 一个 `settings-field` 就是一行。 */
function rowsOf(html: string): string[] {
  const rows: string[] = []
  let at = html.indexOf(FIELD)
  while (at >= 0) {
    const element = elementAt(html, at)
    rows.push(element)
    at = html.indexOf(FIELD, at + element.length)
  }
  return rows
}

/** 某一列（文案列 / 控件列）的内容；这一行没有那一列时返回 `null`。 */
function columnOf(row: string, tag: string): string | null {
  const at = row.indexOf(tag)
  if (at < 0) return null
  const element = elementAt(row, at)
  return element.slice(tag.length, element.length - CLOSE.length)
}

/** 文案列的内容（没有文案列时是空串）。 */
function copyColumn(row: string): string {
  return columnOf(row, COPY) ?? ''
}

/** 控件列的内容；这一行没有控件列时返回 `null`（「没有控件」与「控件是空的」是两件事）。 */
function controlColumn(row: string): string | null {
  return columnOf(row, CONTROL)
}

function rowContaining(html: string, text: string): string {
  const found = rowsOf(html).find((row) => row.includes(text))
  expect(found, `渲染结果里没有包含「${text}」的那一行`).toBeDefined()
  return found ?? ''
}

/** 「随壁纸启动 DSH」开着、壁纸自启又确实读到是关的 —— 警告那一行才出现。 */
function panelWithWarning(autostart: AutostartStatus, page: SettingsPanelProps['page'] = 'connections'): string {
  const settings = normalizeSettings({})
  return renderToStaticMarkup(<SettingsPanel {...settingsPanelProps({
    page,
    settings: { ...settings, dshLaunch: { ...settings.dshLaunch, autoStartWithWallpaper: true } },
    autostart,
  })} />)
}

// `none` + `reason` 就是"确实读到是关的"（`autostartKnown` 只认这一种，占位值不算）。
const NOT_CONFIGURED: AutostartStatus = {
  enabled: false,
  source: 'none',
  reason: '当前用户启动项里没有 DSH Wallpaper。',
}

// 语言是模块级状态：这一组用中文断言（词典是同一份），跑完还原。
beforeEach(() => setLanguage('zh'))
afterEach(() => setLanguage('zh'))

describe('没有控件的那一行（壁纸自启未生效的警告）', () => {
  it('那句话在文案列里，而这一行根本没有控件列', () => {
    const row = rowContaining(panelWithWarning(NOT_CONFIGURED), zh['settings.connections.autostart-warning.title'])
    const copy = copyColumn(row)

    // 正面锚点：标题、说明、那句话三样都在左列里 —— 否则下面"右边没有控件列"会因为整行没渲染而假绿。
    expect(copy).toContain(zh['settings.connections.autostart-warning.title'])
    expect(copy).toContain(zh['settings.connections.autostart-warning.detail'])
    expect(copy).toContain(zh['settings.connections.autostart-warning.not-configured'])
    expect(copy).toContain(NOTE)
    // 没有控件就没有控件列：那句话再也回不到"按 max-content 撑开一列"的位置上。
    expect(controlColumn(row)).toBeNull()
    expect(row).not.toContain(CONTROL)
  })

  it('三种原因的那句话都落在文案列里', () => {
    const cases: Array<[AutostartStatus, string]> = [
      [NOT_CONFIGURED, zh['settings.connections.autostart-warning.not-configured']],
      [{ enabled: false, source: 'disabled-by-user', reason: null }, zh['settings.connections.autostart-warning.disabled-by-user']],
      [{ enabled: false, source: 'disabled-by-policy', reason: null }, zh['settings.connections.autostart-warning.disabled-by-policy']],
    ]
    for (const [status, sentence] of cases) {
      const row = rowContaining(panelWithWarning(status), sentence)
      expect(copyColumn(row), `${status.source} 那句话没在文案列里`).toContain(sentence)
      expect(controlColumn(row), `${status.source} 那一行不该有控件列`).toBeNull()
    }
  })
})

describe('其它用 Field 的地方没有被弄坏', () => {
  const pages = ['general', 'connections', 'appearance', 'history', 'system'] as const

  it('每一行的控件列要么不存在，要么第一个孩子是元素（裸文本就是这次的形态）', () => {
    const collected: string[] = []
    for (const page of pages) {
      // 警告那一行的开关是开着的：扫的必须是"警告在场"的那一版页面 —— 否则这次事故的
      // 那一行根本不在扫描范围里，这条断言会在有 bug 的代码上照样通过。
      collected.push(...rowsOf(panelWithWarning(NOT_CONFIGURED, page)))
    }
    // 正面锚点：真的扫到了很多行（少扫一页，下面的断言就会变成空转）。
    expect(collected.length, '扫到的 Field 行太少 —— 页面没渲染出来，这条断言会变成空转').toBeGreaterThan(30)

    const bareText = collected
      .map((row) => [row, controlColumn(row)] as const)
      .filter(([, control]) => control !== null && control !== '' && !control.startsWith('<'))
    expect(
      bareText.map(([row, control]) => `「${row.slice(0, 40)}…」的控件列以裸文本开头：${(control ?? '').slice(0, 40)}`),
      '一句话被当成控件放进控件列就会这样：那一列按 max-content 定宽、不收缩也不折行',
    ).toEqual([])
  })

  it('有控件的行照旧：文案在左列，控件在右列', () => {
    const html = panelWithWarning(NOT_CONFIGURED)

    // 开关行（就是警告上面那一行）。
    const toggleRow = rowContaining(html, zh['settings.connections.launch-with-wallpaper.title'])
    expect(copyColumn(toggleRow)).toContain(zh['settings.connections.launch-with-wallpaper.title'])
    expect(copyColumn(toggleRow)).toContain(zh['settings.connections.launch-with-wallpaper.checkout'])
    expect(controlColumn(toggleRow) ?? '').toContain('role="switch"')

    // 只有 `<span />` 的行（系统页的工作区目录）：`children` 变的只是"可选"，
    // 给了 children 就照样画控件列 —— 否则那一行的两列布局会塌成左对齐的一列。
    const systemHtml = renderToStaticMarkup(<SettingsPanel {...settingsPanelProps({ page: 'system' })} />)
    const workspaceRow = rowContaining(systemHtml, zh['settings.system.workspace.title'])
    expect(controlColumn(workspaceRow)).toBe('<span></span>')
  })
})

describe('列宽契约（CSS）', () => {
  /** 取一条规则的内容（选择器必须是这个文件里唯一的、写成一行的那种）。 */
  function rule(css: string, selector: string): string {
    const at = css.indexOf(`${selector}{`)
    return at < 0 ? '' : css.slice(at + selector.length + 1, css.indexOf('}', at))
  }

  it('两列都能收缩，控件列不再是 flex: 0 0 auto', async () => {
    const css = await source('src/settings/SettingsPanel.css')

    expect(rule(css, '.settings-field')).toContain('display:flex')
    // 文案列吃掉剩余宽度，而且能收缩 —— 不许再被挤成一个词宽。
    expect(rule(css, '.settings-field__copy')).toContain('min-width:0')
    expect(rule(css, '.settings-field__copy')).toContain('flex:1 1 auto')
    // 控件列必须能收缩：`flex: 0 0 auto` 就是这次塌陷的根因（按 max-content 定宽，撑着不让位）。
    expect(rule(css, '.settings-field__control')).toContain('flex:0 1 auto')
    expect(rule(css, '.settings-field__control')).not.toContain('flex:0 0 auto')
    expect(rule(css, '.settings-field__control')).toContain('min-width:0')
    expect(rule(css, '.settings-field__control')).toContain('flex-wrap:wrap')
    // 长说明在列内换行：路径、URL 这类长 token 也要能断，不许横向溢出卡片。
    expect(rule(css, '.settings-field__copy span')).toContain('overflow-wrap:anywhere')
    // 「那句话」是文案列里的一句（`__note`），不是控件。
    expect(css).toContain('.settings-field__copy .settings-field__note{')
  })
})

describe('警告文案', () => {
  const notes = [
    'settings.connections.autostart-warning.detail',
    'settings.connections.autostart-warning.not-configured',
    'settings.connections.autostart-warning.disabled-by-user',
    'settings.connections.autostart-warning.disabled-by-policy',
  ] as const

  it('英文一句话在列里一行能读完，不再靠溢出表达', () => {
    for (const key of notes) {
      // 列宽 420px（`.settings-field__copy span` 的 max-width）÷ 10px 字号 ≈ 每行 80 来个字符。
      expect(en[key].length, `${key} 的英文又变长了`).toBeLessThanOrEqual(90)
      expect(zh[key].length, `${key} 的中文又变长了`).toBeLessThanOrEqual(60)
      // 改短时删掉的就是这两句：它们把同一件事说了三遍，第二遍开始没人读。
      expect(en[key], `${key} 又把"只有你自己打开壁纸后才生效"写了回来`).not.toContain('only takes effect after you open the wallpaper')
      expect(zh[key], `${key} 又把"只会在你手动打开壁纸后生效"写了回来`).not.toContain('只会在你手动打开壁纸后生效')
    }
  })

  it('指路的那句话指的是真的放着那个开关的那一页', () => {
    // 开关在「系统」页（`page === 'system'` 那张 Windows 集成卡片里的 `settings.system.autostart.title`）。
    // 文案以前指的是「常规」—— 照着做只会找不到开关。
    const systemHtml = renderToStaticMarkup(<SettingsPanel {...settingsPanelProps({ page: 'system' })} />)
    expect(systemHtml, '壁纸自启的开关已经不在「系统」页了，那几句"去系统里开"的话要跟着改')
      .toContain(zh['settings.system.autostart.title'])

    for (const key of [
      'settings.connections.autostart-warning.not-configured',
      'settings.connections.launch-with-wallpaper.shell',
      'settings.connections.launch-with-wallpaper.checkout',
    ] as const) {
      expect(zh[key], `${key} 没指向「${zh['nav.system.label']}」页`).toContain(zh['nav.system.label'])
      expect(en[key], `${key} does not point at the "${en['nav.system.label']}" page`).toContain(en['nav.system.label'])
      expect(zh[key], `${key} 还指着「${zh['nav.general.label']}」页`).not.toContain(zh['nav.general.label'])
    }
  })
})
