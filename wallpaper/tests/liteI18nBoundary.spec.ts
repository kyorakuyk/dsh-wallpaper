import { readFile, stat } from 'node:fs/promises'
import { dirname, posix, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { FULL_DICTIONARIES } from '../src/i18n/full.ts'
import { setLanguage, t } from '../src/i18n/index.ts'
import { LITE_DICTIONARIES } from '../src/i18n/lite.ts'
import { zhShared } from '../src/i18n/zh.shared.ts'

/**
 * Lite 产物的词条边界：**Lite 只说它自己那几句话**。
 *
 * 为什么要有这一组：词条曾经整本由 `i18n/index.ts` 持有，而 Lite 入口也 import 它，于是完整版的
 * 每一条键名与文案都进了 `dist-lite` 的 bundle —— `scripts/verify-lite-bundle.ps1` 在 CI 上拦住的
 * 正是这件事（产物里出现 `DeepSeek Harness`、`会话生命周期`、`deepseek-web` 这些字样）。拆开之后，
 * "哪些键属于 Lite"不能靠人记：这里走一遍**入口可达的 import 图**，把 Lite 真的会用到的键扫出来，
 * 与登记的那一份对齐。
 *
 * 三条契约：
 * 1. `lite ⊆ full`（键集合），两种语言同键；
 * 2. Lite 可达的模块**只引用** shared 里的键 —— 想加一句完整版的话，先把它的词条搬进 `zh.shared.ts`；
 * 3. shared 里没有 Lite 用不到的键 —— 多留一条就是往 Lite 产物里塞一句没人念的文案。
 *
 * 扫的是**字符串字面量**（键名都是字面量写的）。拼出来的键（带插值的模板串）扫描看不见，那种键会被
 * 第 3 条等式拦下来（"这条键没人用"）—— 本仓库没有这种写法，真出现了这里就要一起升级。
 */
const wallpaperRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

const readSource = async (relative: string): Promise<string> =>
  (await readFile(resolve(wallpaperRoot, relative), 'utf8')).replace(/\r\n?/g, '\n')

/** 键名的形状：至少一段小写点分。先按形状筛出"像键的字面量"，再与全量词典求交集。 */
const KEY_SHAPE = /^[a-z][A-Za-z0-9]*(?:\.[A-Za-z0-9-]+)+$/

const fullKeys: string[] = Object.keys(FULL_DICTIONARIES.zh)
const liteKeys: string[] = Object.keys(LITE_DICTIONARIES.zh)

afterEach(() => {
  setLanguage('zh')
  vi.restoreAllMocks()
})

function parse(fileName: string, text: string): ts.SourceFile {
  const kind = fileName.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  return ts.createSourceFile(fileName, text, ts.ScriptTarget.ES2020, true, kind)
}

/** 整串具名 import 都是 `type` 时（`import { type A, type B }`），这条 import 编译后就没了。 */
function allTypeOnly(namedBindings: ts.NamedImportBindings | ts.NamedExportBindings | undefined): boolean {
  if (namedBindings === undefined) return false
  const elements = ts.isNamedImports(namedBindings) || ts.isNamedExports(namedBindings) ? namedBindings.elements : []
  return elements.length > 0 && elements.every((element) => element.isTypeOnly)
}

/**
 * 一个模块的**值依赖**：`import type` / 全 `type` 具名 import 编译后就被擦掉，进不了产物的图。
 */
function valueImports(file: ts.SourceFile): string[] {
  const specifiers: string[] = []
  const visit = (node: ts.Node): void => {
    if (ts.isImportDeclaration(node)) {
      const clause = node.importClause
      const value = clause === undefined || (!clause.isTypeOnly && !allTypeOnly(clause.namedBindings))
      if (value && ts.isStringLiteral(node.moduleSpecifier)) specifiers.push(node.moduleSpecifier.text)
    } else if (ts.isExportDeclaration(node)) {
      const value = !node.isTypeOnly && !allTypeOnly(node.exportClause)
      if (value && node.moduleSpecifier !== undefined && ts.isStringLiteral(node.moduleSpecifier)) {
        specifiers.push(node.moduleSpecifier.text)
      }
    } else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
      const first = node.arguments[0]
      if (first !== undefined && ts.isStringLiteral(first)) specifiers.push(first.text)
    }
    ts.forEachChild(node, visit)
  }
  visit(file)
  return specifiers
}

/** 模块里出现的所有字符串字面量（没有插值的模板串也算）。 */
function stringLiterals(file: ts.SourceFile): Set<string> {
  const found = new Set<string>()
  const visit = (node: ts.Node): void => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) found.add(node.text)
    ts.forEachChild(node, visit)
  }
  visit(file)
  return found
}

/** 相对 `src/` 的模块路径（用正斜杠，跟着 import 里写的那种）。 */
async function resolveModule(fromRelative: string, specifier: string): Promise<string | undefined> {
  const base = posix.normalize(posix.join(posix.dirname(fromRelative), specifier))
  for (const candidate of [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`]) {
    try {
      if ((await stat(resolve(wallpaperRoot, 'src', candidate))).isFile()) return candidate
    } catch {
      // 换下一个候选
    }
  }
  return undefined
}

/**
 * Lite 入口可达的模块（相对 `src/` 的路径 -> 源码）。
 *
 * 这是"Lite 的产物里会有什么"在源码侧的对应物：Vite 也是从那一个 HTML 入口出发顺着值依赖打包。
 * 走不通的相对路径**直接报错**而不是跳过 —— 漏一个模块，这一组契约就静默地少查一截。
 */
async function liteModuleGraph(): Promise<Map<string, string>> {
  const graph = new Map<string, string>()
  const queue = ['main-lite.tsx']
  while (queue.length > 0) {
    const relativePath = queue.pop()!
    if (graph.has(relativePath)) continue
    const text = await readSource(`src/${relativePath}`)
    graph.set(relativePath, text)
    for (const specifier of valueImports(parse(relativePath, text))) {
      if (specifier.endsWith('.css')) continue
      const resolved = await resolveModule(relativePath, specifier)
      if (resolved !== undefined) queue.push(resolved)
      else if (specifier.startsWith('.')) throw new Error(`${relativePath} 里的相对 import 解析不了：${specifier}`)
    }
  }
  return graph
}

/** 词典文件本身不是"引用"：键就是在那里定义的。 */
const isDictionary = (relativePath: string): boolean => /^i18n\/(zh|en)\./.test(relativePath)

/**
 * 入口的**第一条** import。
 *
 * 登记必须排在那里：ES 模块按 import 顺序求值，而后面那些模块在 import 期就可能要句子
 * （`persona/registry.ts` 这类表在 import 时建好，名字靠 getter 现取，但谁也保不齐以后多一句
 * 模块级的 `t()`）。排在后面 = 那句话在登记之前求值 = 界面上出现键名。
 */
function firstImport(text: string): string | undefined {
  for (const statement of parse('entry.tsx', text).statements) {
    if (ts.isImportDeclaration(statement) && ts.isStringLiteral(statement.moduleSpecifier)) {
      return statement.moduleSpecifier.text
    }
  }
  return undefined
}

/** Lite 可达的模块里**按字面量引用的键**（词典文件不算），带上是哪个模块引用的，报错时好找。 */
async function referencedKeys(): Promise<Map<string, string>> {
  const graph = await liteModuleGraph()
  const declared = new Set(fullKeys)
  const referenced = new Map<string, string>()
  for (const [relativePath, text] of graph) {
    if (isDictionary(relativePath)) continue
    for (const literal of stringLiterals(parse(relativePath, text))) {
      if (!KEY_SHAPE.test(literal) || !declared.has(literal)) continue
      if (!referenced.has(literal)) referenced.set(literal, relativePath)
    }
  }
  return referenced
}

describe('Lite 的词条边界', () => {
  it('Lite 比完整版小，是完整的子集，而且两种语言同键', () => {
    expect(liteKeys.length).toBeGreaterThan(0)
    expect(liteKeys.length).toBeLessThan(fullKeys.length)
    expect(
      liteKeys.filter((key) => !fullKeys.includes(key)),
      '这些键在 Lite 里、却不在完整版里 —— 拆词条时拆坏了',
    ).toEqual([])
    expect(Object.keys(LITE_DICTIONARIES.en).sort()).toEqual([...liteKeys].sort())
    expect(Object.keys(FULL_DICTIONARIES.en).sort()).toEqual([...fullKeys].sort())
  })

  it('完整版的词典不在 Lite 的图里', async () => {
    const graph = await liteModuleGraph()
    // 机制（`i18n/index.ts`）不认识任何词典，Lite 只登记 shared —— 少哪一样，完整版的键名与文案
    // 都会跟着进 `dist-lite`。
    expect([...graph.keys()].filter(isDictionary).sort()).toEqual(['i18n/en.shared.ts', 'i18n/zh.shared.ts'])
    expect([...graph.keys()].filter((file) => /^i18n\/.*\.ts$/.test(file)).sort()).toEqual([
      'i18n/en.shared.ts',
      'i18n/index.ts',
      'i18n/lite.ts',
      'i18n/zh.shared.ts',
    ])
    // 两个入口各自登记自己那一份，而且必须是**第一条** import（见 `firstImport` 的说明）。
    const liteEntry = await readSource('src/main-lite.tsx')
    expect(firstImport(liteEntry)).toBe('./i18n/lite.ts')
    expect(liteEntry).not.toContain('i18n/full')
    expect(firstImport(await readSource('src/main.tsx'))).toBe('./i18n/full.ts')
  })

  it('Lite 可达的模块只引用 shared 里的键', async () => {
    const referenced = await referencedKeys()
    const lite = new Set(liteKeys)
    const leaked = [...referenced]
      .filter(([key]) => !lite.has(key))
      .map(([key, file]) => `${key}（${file}）`)
    expect(
      leaked,
      '这些键只有完整版的词典里有，却被 Lite 入口可达的模块引用了：把词条搬进 zh.shared.ts / en.shared.ts'
        + '（Lite 会说这句话，就该有这条词条），或者把那段代码从 Lite 可达的路径里挪开',
    ).toEqual([])
  })

  it('shared 里没有 Lite 用不到的键', async () => {
    const referenced = await referencedKeys()
    const unused = liteKeys.filter((key) => !referenced.has(key))
    expect(
      unused,
      '这些键在 Lite 的可达模块里没有被用到：搬去 zh.full.ts / en.full.ts，别让它们占着 Lite 的产物',
    ).toEqual([])
  })

  it('缺词条时不崩、不留空白，只回落成键名', async () => {
    // 机制与登记必须是两件事：**只** import 机制时注册表是空的（否则机制里还藏着词典）。
    vi.resetModules()
    const bare = await import('../src/i18n/index.ts')
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(bare.t('lite.settings.hero.title')).toBe('lite.settings.hero.title')
    expect(warn).toHaveBeenCalled()

    // 登记 Lite 那一份之后：Lite 说得上的句子照旧跟着语言走，完整版才有的键回落成键名。
    await import('../src/i18n/lite.ts')
    expect(bare.t('lite.settings.hero.title')).toBe(zhShared['lite.settings.hero.title'])
    bare.setLanguage('en')
    expect(bare.t('lite.settings.hero.title')).not.toBe(zhShared['lite.settings.hero.title'])
    expect(bare.t('lite.settings.hero.title')).not.toBe('')
    bare.setLanguage('zh')
    const fullOnly = bare.t('settings.general.interaction.title')
    expect(fullOnly).toBe('settings.general.interaction.title')
    expect(fullOnly).not.toBe('')
  })
})

describe('单测里的登记', () => {
  it('走的是完整版那一份（`tests/i18nSetup.ts` 与 `src/main.tsx` 是同一件事）', () => {
    // 没有这一行登记，`t()` 只有机制：每条断言都会看到键名，而不是句子。
    expect(t('language.label')).toBe(FULL_DICTIONARIES.zh['language.label'])
    expect(t('settings.general.interaction.title')).toBe(FULL_DICTIONARIES.zh['settings.general.interaction.title'])
  })
})
