import { describe, expect, it } from 'vitest'
import { DESKTOP_WORKSPACE_DIRECTORY_NAME, WALLPAPER_DATA_DIRECTORY_NAME, defaultDesktopWorkspacePath } from '../src/index.ts'

const config = { tokenRoot: 'C:\\Users\\me\\.dsh' } as never

/**
 * 「桌面会话」的目录位置。
 *
 * 用户的要求是"放进 dsh-wallpaper 打包后自己的目录里"，并且**卸载也要保留**。三条约束叠在一起
 * 只剩下一个选择：
 *
 * - 安装目录不行 —— 那是 `C:\Program Files\WindowsApps\com.dsh.wallpaper_<version>_...`，
 *   每次升级整个被替换，写在那里的东西必丢；
 * - DSH 那边或系统临时目录也不行 —— 不是"壁纸自己的目录"，卸载/清理时说不清该不该留；
 * - **数据目录**（`%LOCALAPPDATA%\com.dsh.wallpaper`）刚好：运行时可写、升级保留、
 *   卸载后仍留得下（普通用户目录，MSIX 不管它）。
 */
describe('the desktop session workspace location', () => {
  it('lives in the wallpaper data directory, named like its DSH title', () => {
    const path = defaultDesktopWorkspacePath(config, 'C:\\Users\\me\\AppData\\Local')
    expect(path).toBe(`C:\\Users\\me\\AppData\\Local\\${WALLPAPER_DATA_DIRECTORY_NAME}\\${DESKTOP_WORKSPACE_DIRECTORY_NAME}`)
    expect(path.endsWith('桌面会话')).toBe(true)
    // 绝不能落在安装位置：那目录每次升级都被替换。
    expect(path.toLowerCase()).not.toContain('windowsapps')
    expect(path.toLowerCase()).not.toContain('program files')
  })

  it('still answers a usable path when the environment variable is missing', () => {
    const path = defaultDesktopWorkspacePath(config, undefined)
    expect(path).toContain(DESKTOP_WORKSPACE_DIRECTORY_NAME)
    expect(path).toContain('wallpaper')
  })
})
