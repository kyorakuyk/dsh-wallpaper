import { describe, expect, it } from 'vitest'
import { openRoutesFor, selectedOpenRoute, type OpenRouteInput } from '../src/connect/openRoutes.ts'

function input(patch: Partial<OpenRouteInput>): OpenRouteInput {
  return { hasSubject: true, shellSelected: false, subjectKind: 'installed-cli', tuiAvailable: true, ...patch }
}

describe('open routes', () => {
  it('offers nothing at all before a subject is chosen', () => {
    expect(openRoutesFor(input({ hasSubject: false }))).toEqual([])
  })

  it('offers the terminal for an installed CLI only when a TUI is actually installed', () => {
    expect(openRoutesFor(input({ tuiAvailable: true })).map((route) => route.value)).toEqual(['browser', 'tui'])
    // 没装 TUI：只剩浏览器。那个选项点下去只会报"本机没有找到 TUI"，不该摆出来。
    expect(openRoutesFor(input({ tuiAvailable: false })).map((route) => route.value)).toEqual(['browser'])
  })

  it('gives the shell its own single route, because it has one window of its own', () => {
    const routes = openRoutesFor(input({ shellSelected: true, subjectKind: 'embedded-shell' }))
    expect(routes).toEqual([{ value: 'browser', label: '官方客户端窗口' }])
  })

  it('gives a source tree a single route, TUI or not', () => {
    expect(openRoutesFor(input({ subjectKind: 'checkout' })).map((route) => route.value)).toEqual(['browser'])
    expect(openRoutesFor(input({ subjectKind: 'checkout', tuiAvailable: false }))).toHaveLength(1)
  })
})

describe('the stored route', () => {
  it('is honoured only while there really are two ways to open', () => {
    const both = openRoutesFor(input({ tuiAvailable: true }))
    expect(selectedOpenRoute(both, 'tui')).toBe('tui')
    expect(selectedOpenRoute(both, 'browser')).toBe('browser')
    expect(selectedOpenRoute(both, undefined)).toBe('browser')
  })

  it('falls back to the browser once the TUI is gone, instead of leaving a dead choice', () => {
    // 档案里存着 tui，但机器上已经没有 TUI 了：选项只剩一条，于是回落。
    const onlyBrowser = openRoutesFor(input({ tuiAvailable: false }))
    expect(selectedOpenRoute(onlyBrowser, 'tui')).toBe('browser')
  })
})
