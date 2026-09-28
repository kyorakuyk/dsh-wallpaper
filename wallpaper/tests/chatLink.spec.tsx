import { readFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { MarkdownBody } from '../src/features/chat/MarkdownBody.tsx'
import { isExternalLink, parseInline } from '../src/features/chat/markdown.ts'

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'src')
const bodySource = readFile(resolve(sourceRoot, 'features/chat/MarkdownBody.tsx'), 'utf8')

/**
 * 链接的三件事必须同时成立，缺一件就会在真实桌面上出事故：
 *
 *  1. 只有 http/https 会变成链接，别的地址保持字面（模型输出不可信）；
 *  2. **左键只 preventDefault，不打开** —— 真的 `<a href>` 一旦被激活，WebView2 会把整张壁纸
 *     导航走，桌面就变成了那个网页；
 *  3. 打开只有中键与回车两个手势，且悬停/聚焦时在文字下方露出真实地址。
 */
describe('transcript links', () => {
  it('styles only addresses it is allowed to hand to the shell', () => {
    expect(isExternalLink('https://www.markdownguide.org/')).toBe(true)
    expect(isExternalLink('http://127.0.0.1:3080/a?b=c')).toBe(true)
    for (const url of [
      'javascript:alert(1)',
      'file:///C:/Windows/System32/calc.exe',
      'ms-settings:startupapps',
      'www.example.com',
      'https://аpple.com/',
      'https://apple.com@evil.test/',
      'https://exa mple.com/',
      `https://example.com/${'a'.repeat(4096)}`,
    ]) {
      expect(isExternalLink(url), url).toBe(false)
    }
  })

  it('keeps a link literal when the address is not openable', () => {
    expect(parseInline('[官方指南](https://www.markdownguide.org/)')).toEqual([
      { kind: 'link', href: 'https://www.markdownguide.org/', children: [{ kind: 'text', text: '官方指南' }] },
    ])
    // 不合规 → 整段字面，与"没点名的语法保持字面"同一条规矩。
    expect(parseInline('[点我](javascript:alert(1))')).toEqual([
      { kind: 'text', text: '[点我](javascript:alert(1))' },
    ])
    // 图片语法刻意不支持：`!` 开头不是链接。
    expect(parseInline('![图](https://example.com/x.png)')).toEqual([
      { kind: 'text', text: '![图](https://example.com/x.png)' },
    ])
    // 链接文字里照旧允许行内标记。
    expect(parseInline('[**粗**的指南](https://example.com/)')).toEqual([
      {
        kind: 'link',
        href: 'https://example.com/',
        children: [
          { kind: 'bold', children: [{ kind: 'text', text: '粗' }] },
          { kind: 'text', text: '的指南' },
        ],
      },
    ])
  })

  it('renders an anchor that carries the address twice, visibly and for the tooltip', async () => {
    const html = renderToStaticMarkup(<MarkdownBody text="见 [官方指南](https://www.markdownguide.org/) 。" />)
    // 链接本体
    expect(html).toContain('href="https://www.markdownguide.org/"')
    expect(html).toContain('class="dsh-chat__link"')
    expect(html).toContain('官方指南')
    // 地址那一行：真元素（可选中、复制），并且对辅助技术隐藏（链接自己的 href 已经报了去处）。
    expect(html).toContain('class="dsh-chat__link-url" aria-hidden="true"')
    // title 把手势写出来：中键不是能猜出来的交互。
    expect(html).toContain('中键或回车打开')
  })

  it('never lets a left click navigate the wallpaper away', async () => {
    const body = await bodySource
    // 左键：只拦导航，不打开。
    expect(body).toContain('onClick={(event) => event.preventDefault()}')
    // 中键：打开；按下时还要拦掉 Chromium 的自动滚动。
    expect(body).toMatch(/onAuxClick=\{\(event\) => \{[\s\S]*?event\.button !== 1[\s\S]*?onOpenLink\?\.\(token\.href\)/)
    expect(body).toMatch(/onMouseDown=\{\(event\) => \{[\s\S]*?event\.button === 1[\s\S]*?preventDefault\(\)/)
    // 回车：链接自己合成的 click 已被 preventDefault 吃掉，所以打开只写在 keydown 里。
    expect(body).toMatch(/onKeyDown=\{\(event\) => \{[\s\S]*?event\.key !== 'Enter'[\s\S]*?onOpenLink\?\.\(token\.href\)/)
  })

  it('does nothing at all when the surface passes no opener', () => {
    // 设置中心里也会渲染 MarkdownBody 吗？现在不会；但即使渲染，没有回调时它只是显示成链接。
    const html = renderToStaticMarkup(<MarkdownBody text="[指南](https://example.com/)" />)
    expect(html).toContain('href="https://example.com/"')
  })
})
