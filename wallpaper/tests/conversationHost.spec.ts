import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'
import { conversationHostChip } from '../src/connect/conversationHost.ts'
import { en } from '../src/i18n/en.ts'
import { setLanguage } from '../src/i18n/index.ts'
import { zh } from '../src/i18n/zh.ts'

const SHELL = 'shell:com.deepseek.dsh'
const CLI = 'cli:C:\\Users\\someone\\AppData\\Roaming\\npm\\dsh.cmd'
const CHECKOUT = 'D:\\Family\\DeepSeekHarness\\deepseek-harness'

const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const dictionarySource = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

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

// 语言是模块级状态，测试之间必须还原，否则先跑的那条会决定后跑的那条看到什么。
afterEach(() => setLanguage('zh'))

/**
 * 岛左下角那枚指示器。它要回答用户的两次"分不清"：滑槽左端是免费网页还是自己的 API key，
 * 右侧拉起的是哪个界面。所以这里本来有两条断言主线 —— 花钱的事必须说出来，界面的种类必须与
 * 设置中心那张「打开」卡片同源。
 *
 * **花钱那一条已经改了向**（用户要求界面上不再出现"计价"）：徽章现在只说明"这条通道是谁"，
 * 钱的事一个字都不说。改向的部分在下面「宿主徽章不再提钱之后」那一组里，界面的种类照旧。
 */
describe('the island host indicator', () => {
  it('names the channel on the slider\u2019s left end, and says nothing about money', () => {
    const web = conversationHostChip({ backend: 'deepseek-web', subjectId: undefined, window: undefined })
    expect(web.text).toBe('Web')
    // 身份说明留着 —— 冻的是"钱"，不是这枚徽章。
    expect(web.title).toContain('网页额度')
    expect(web.title).not.toContain('费用')
    expect(web.title).not.toContain('计费')

    const api = conversationHostChip({ backend: 'deepseek-api', subjectId: undefined, window: undefined })
    expect(api.text).toBe('API')
    expect(api.title).toContain('你自己的 DeepSeek API key')
    expect(api.title).not.toContain('计费')
    expect(api.title).not.toContain('费用')
  })

  it('names which interface a harness run brings', () => {
    expect(conversationHostChip({ backend: 'harness', subjectId: SHELL, window: 'browser' }).text).toBe('Desktop')
    expect(conversationHostChip({ backend: 'harness', subjectId: CLI, window: 'browser' }).text).toBe('Web')
    expect(conversationHostChip({ backend: 'harness', subjectId: CLI, window: 'tui' }).text).toBe('TUI')
    expect(conversationHostChip({ backend: 'harness', subjectId: CHECKOUT, window: 'browser' }).text).toBe('Web')
  })

  it('keeps the TUI route to the class that actually has one', () => {
    // 只有「已安装的 CLI」真的有两条路（见 SettingsPanel 的 openRoutes）：客户端与源码目录即便
    // 设置里写着 tui，也仍然走它们唯一的那条路。这里必须和那张卡片一致，不然指示器会说谎。
    expect(conversationHostChip({ backend: 'harness', subjectId: SHELL, window: 'tui' }).text).toBe('Desktop')
    expect(conversationHostChip({ backend: 'harness', subjectId: CHECKOUT, window: 'tui' }).text).toBe('Web')
    // 没选主体（旧设置只有路径，或用户还没配）时按源码目录处理，与 `subjectKindOf` 同一条兜底。
    expect(conversationHostChip({ backend: 'harness', subjectId: undefined, window: 'tui' }).text).toBe('Web')
  })

  it('reuses the subject vocabulary instead of inventing a second one', () => {
    expect(conversationHostChip({ backend: 'harness', subjectId: CHECKOUT, window: 'browser' }).title).toContain('源码目录')
    expect(conversationHostChip({ backend: 'harness', subjectId: CLI, window: 'tui' }).title).toContain('已安装的 CLI')
    expect(conversationHostChip({ backend: 'harness', subjectId: CLI, window: 'browser' }).title).toContain('已安装的 CLI')
    expect(conversationHostChip({ backend: 'harness', subjectId: SHELL, window: 'browser' }).title).toContain('客户端')
  })

  it('adds an alias only when the user set one, and only for a harness subject', () => {
    const checkout = { backend: 'harness' as const, subjectId: CHECKOUT, window: 'browser' as const }
    expect(conversationHostChip(checkout).title).not.toContain('别名')
    expect(conversationHostChip({ ...checkout, alias: '   ' }).title).not.toContain('别名')
    // 两端空白要去掉：别名是用户手里的输入框，没道理让空格进到显示文本里。
    expect(conversationHostChip({ ...checkout, alias: ' 工作机 ' }).title).toContain('（别名：工作机）')
    // 网页与 API 后端没有主体可指认，别名在那里没有意义。
    expect(conversationHostChip({ backend: 'deepseek-web', subjectId: CLI, window: 'browser', alias: '工作机' }).title).not.toContain('工作机')
  })

  it('never leaks the raw subject id or a path into the indicator', () => {
    for (const subjectId of [SHELL, CLI, CHECKOUT]) {
      const chip = conversationHostChip({ backend: 'harness', subjectId, window: 'browser' })
      expect(chip.text).not.toContain(subjectId)
      expect(chip.title).not.toContain(subjectId)
      expect(chip.title).not.toContain('\\')
    }
  })
})

/**
 * 宿主徽章不再提钱之后（用户要求界面上不再出现"计价"）。
 *
 * 上面那两条渲染断言只能证明"这一次取到的文案里没有钱"。这一组钉的是**词条与源码的形状**：
 * 删掉的半句必须逐字留在词条上方的 FREEZE 注释里（恢复时不必翻 git 历史），**中英两侧**都要
 * 留；而活着的词条值里不许再出现任何花钱的字眼 —— 两侧一起钉，否则只改一侧就会漏。
 *
 * 写法与 `ConversationBubble.spec.tsx` 的「用量与费用读数冻结之后」同源：不是把断言删掉，
 * 而是把"必须说出花钱的事"换成"必须已冻结、且留了复活的话"。
 */
describe('宿主徽章不再提钱之后', () => {
  /** 被删掉的那半句：词条键 + 原文（逐字），恢复时照着接回去就行。 */
  const deleted = [
    {
      language: 'zh',
      file: 'src/i18n/zh.ts',
      originals: [
        "'chat.host.deepseek-api': '你自己的 DeepSeek API key，按 token 计费。'",
        "'chat.host.deepseek-web': 'DeepSeek 网页额度，不产生 API 费用。'",
      ],
      money: /计费|费用/,
      restore: '怎么恢复',
    },
    {
      language: 'en',
      file: 'src/i18n/en.ts',
      originals: [
        "'chat.host.deepseek-api': 'Your own DeepSeek API key, billed per token.'",
        "'chat.host.deepseek-web': 'DeepSeek Web quota; no API charges.'",
      ],
      money: /billed|charges|price|cost/i,
      restore: 'Restore',
    },
  ] as const

  it('drops the money clause in both languages, because the badge reads in whichever is on screen', () => {
    // 中文（默认语言）那一侧由上一条钉过，这里把英文那一侧补齐：同一枚徽章、同一句身份说明，
    // 换语言之后仍然一个字都不提钱。
    setLanguage('en')
    const web = conversationHostChip({ backend: 'deepseek-web', subjectId: undefined, window: undefined })
    expect(web.title).toContain('DeepSeek Web quota')
    expect(web.title).not.toMatch(/billed|charges|price|cost/i)

    const api = conversationHostChip({ backend: 'deepseek-api', subjectId: undefined, window: undefined })
    expect(api.title).toContain('Your own DeepSeek API key')
    expect(api.title).not.toMatch(/billed|charges|price|cost/i)
  })

  it('leaves no live entry that prices the channel, in either dictionary', () => {
    for (const [language, dictionary, money] of [
      ['zh', zh, /计费|费用/],
      ['en', en, /billed|charges|price|cost/i],
    ] as const) {
      for (const key of ['chat.host.deepseek-api', 'chat.host.deepseek-web'] as const) {
        expect(dictionary[key], `${language} 的 ${key} 又在提钱了`).not.toMatch(money)
        expect(dictionary[key].length, `${language} 的 ${key} 被清空了`).toBeGreaterThan(0)
      }
    }
  })

  it('keeps the deleted half-sentences in FREEZE notes, so reviving them needs no git archaeology', async () => {
    for (const { language, file, originals, restore } of deleted) {
      const source = await dictionarySource(file)
      expect(source, `${language} 的注释里没有 FREEZE`).toContain('FREEZE')
      expect(source, `${language} 的注释里没写恢复办法`).toContain(restore)
      for (const original of originals) {
        // 原文逐字留着（连词条键一起，照着接回去就行）……
        expect(source, `${language} 没有留下被删掉的原文：${original}`).toContain(original)
        // ……而且必须**在注释里**：剥掉注释之后它不能还活着，否则徽章会重新提钱。
        expect(withoutComments(source), `${language} 里那句原文又活过来了：${original}`).not.toContain(original)
      }
    }
  })
})
