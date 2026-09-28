import { describe, expect, it } from 'vitest'
import { subjectOptionLabel } from '../src/connect/harnessSubjects.ts'
import type { HarnessTarget } from '../src/native/runtime.ts'

/**
 * 「运行方式」下拉里每一行的文字，包括版本那一段。
 *
 * 为什么这一行需要版本：客户端自带的运行时是 0.2.0-rc.1，而用户自己那棵树可能是 0.1.0-rc.5，
 * 两者都叫 deepseek harness，但能不能接上同一个 bridge 并不是同一个答案。所以每条后面跟它**自己**
 * 的版本号；读不到版本时这一行**一个字都不加** —— 写"未知"会让用户以为我们查过这一条。
 */
function subject(overrides: Partial<HarnessTarget> = {}): HarnessTarget {
  return {
    id: 'shell:com.deepseek.dsh',
    kind: 'embedded-shell',
    client: 'official-desktop',
    label: '官方桌面客户端',
    source: 'C:\\Users\\someone\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs',
    identity: { aumid: 'com.deepseek.dsh', defaultPorts: [19387] },
    launch: { kind: 'apps-folder', alias: 'shell:AppsFolder\\com.deepseek.dsh' },
    capabilities: {
      singleInstance: true,
      ownsWindow: true,
      canStartHidden: true,
      needsProfile: false,
    },
    ...overrides,
  }
}

/** 一棵源码树，按扫描给出的形状（路径就是身份，版本来自它自己的清单）。 */
function checkout(rootPath: string, version?: string): HarnessTarget {
  return {
    id: rootPath,
    kind: 'checkout',
    client: 'official-web',
    label: 'deepseek-harness',
    version,
    source: '磁盘扫描',
    identity: { rootPath, defaultPorts: [3080] },
    launch: { kind: 'managed-command' },
    capabilities: {
      singleInstance: false,
      ownsWindow: false,
      canStartHidden: true,
      needsProfile: true,
    },
  }
}

/** 全局安装的 DSH CLI，按扫描给出的形状（启动器就是身份，版本来自 npm 全局包清单）。 */
function installedCli(version?: string): HarnessTarget {
  const launcher = 'C:\\Users\\someone\\AppData\\Roaming\\npm\\dsh.cmd'
  return {
    id: `cli:${launcher}`,
    kind: 'installed-cli',
    client: 'official-web',
    label: 'DSH CLI',
    version,
    source: launcher,
    identity: { defaultPorts: [3080] },
    launch: { kind: 'managed-command' },
    capabilities: {
      singleInstance: false,
      ownsWindow: false,
      canStartHidden: true,
      needsProfile: true,
    },
  }
}

describe('the version segment of one 运行方式 option', () => {
  it('follows the name the user reads, for all three kinds', () => {
    // 客户端与 CLI 的名字本身就说清了它们是什么，所以不冠类别词（用户实测点名：冠了就是同一件事说两遍）。
    const shell = subject({ version: '0.2.0-rc.1' })
    expect(subjectOptionLabel(shell, [shell])).toBe('官方桌面客户端 · 0.2.0-rc.1')

    // 源码目录的名字只是个仓库名，必须带类别词；只有一棵时不必区分，就用仓库名本身。
    const tree = checkout('D:\\Family\\DeepSeekHarness\\deepseek-harness', '0.1.0-rc.5')
    expect(subjectOptionLabel(tree, [tree])).toBe('源码目录 · deepseek-harness · 0.1.0-rc.5')

    const cli = installedCli('0.2.0-rc.1')
    expect(subjectOptionLabel(cli, [cli])).toBe('DSH CLI · 0.2.0-rc.1')
  })

  it('is left out entirely when the scan could not read one', () => {
    // 没有这个字段：标签就是今天的样子，一个字符都没变。
    const unknown = subject()
    expect(subjectOptionLabel(unknown, [unknown])).toBe('官方桌面客户端')
    // 空白版本与没有版本是同一件事：不留一段空的 ` · `，也不写占位符。
    const blank = subject({ version: '   ' })
    expect(subjectOptionLabel(blank, [blank])).toBe('官方桌面客户端')
    // 两条都没有版本，仍然是两条可区分的条目（靠目录名那一段）。
    const left = checkout('D:\\Family\\DeepSeekHarness\\deepseek-harness')
    const right = checkout('C:\\DeepSeekHarness.old\\deepseek-harness')
    expect(subjectOptionLabel(left, [left, right])).toBe('源码目录 · DeepSeekHarness')
    expect(subjectOptionLabel(right, [left, right])).toBe('源码目录 · DeepSeekHarness.old')
  })

  it('keeps two trees of the same name apart with their own versions', () => {
    // 真实情形：同一个项目克隆了两次，各自停在不同版本上 —— 这正是版本段最能帮上忙的地方。
    const left = checkout('D:\\Family\\DeepSeekHarness\\deepseek-harness', '0.1.0-rc.5')
    const right = checkout('C:\\DeepSeekHarness.old\\deepseek-harness', '0.2.0-rc.1')
    expect(subjectOptionLabel(left, [left, right])).toBe(
      '源码目录 · DeepSeekHarness · 0.1.0-rc.5',
    )
    expect(subjectOptionLabel(right, [left, right])).toBe(
      '源码目录 · DeepSeekHarness.old · 0.2.0-rc.1',
    )
  })
})
