import { describe, expect, it } from 'vitest'
import { conversationHostChip } from '../src/connect/conversationHost.ts'

const SHELL = 'shell:com.deepseek.dsh'
const CLI = 'cli:C:\\Users\\someone\\AppData\\Roaming\\npm\\dsh.cmd'
const CHECKOUT = 'D:\\Family\\DeepSeekHarness\\deepseek-harness'

/**
 * 岛左下角那枚指示器。它要回答用户的两次"分不清"：滑槽左端是免费网页还是自己的 API key，
 * 右侧拉起的是哪个界面。所以这里有两条断言主线 —— 花钱的事必须说出来，界面的种类必须与
 * 设置中心那张「打开」卡片同源。
 */
describe('the island host indicator', () => {
  it('settles the money question on the slider\u2019s left end', () => {
    const web = conversationHostChip({ backend: 'deepseek-web', subjectId: undefined, window: undefined })
    expect(web.text).toBe('Web')
    expect(web.title).toContain('网页额度')
    expect(web.title).toContain('不产生 API 费用')

    const api = conversationHostChip({ backend: 'deepseek-api', subjectId: undefined, window: undefined })
    expect(api.text).toBe('API')
    expect(api.title).toContain('你自己的 DeepSeek API key')
    expect(api.title).toContain('计费')
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
