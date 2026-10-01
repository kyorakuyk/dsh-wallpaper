/** 形态（persona）类型定义 —— 立绘/插画/动画/气泡的定制化核心 */

import { t } from '../i18n/index.ts'

export type PersonaId = string

export type AgeKind = 'child' | 'adult'

export type ThemeKind = 'blue' | 'black'

export interface PersonaTheme {
  /** 主题主色（驱动气泡/边框/氛围色） */
  primary: string
  /** 主题辅色 */
  accent: string
  /** 背景氛围色（用于透明壁纸下的氛围光） */
  glow: string
}

export interface PersonaBubbles {
  /** 待机问候 */
  morning: string
  /** 任务完成 */
  done: string
  /** DSH 上线提示 */
  harnessOnline: string
  /** DSH 下线提示 */
  harnessOffline: string
  /** 会话打开时 */
  chatOpen: string
}

export interface PersonaAnimations {
  /** 帧路径（相对 persona 目录）与 fps；缺省则用静态帧 */
  sleep?: { frames: string[]; fps?: number }
  wake?: { frames: string[]; fps?: number }
  idle?: { frames: string[]; fps?: number }
}

export interface PersonaManifest {
  id: PersonaId
  name: string
  theme: PersonaTheme
  age: AgeKind
  /** 形态标签：蓝=deepseek.com 后端；黑=DSH(3080) 后端 */
  kind: ThemeKind
  animations?: PersonaAnimations
  bubbles: PersonaBubbles
  /** 素材相对路径（相对 assets/personas/<id>/ 或 public 绝对路径 /personas/...） */
  assets: {
    portrait: string // 立绘（透明底 PNG / 竖版图）
    illustration?: string // 背景插画
    sleep?: string // 睡眠静态图（缺省用程序占位）
    wake?: string // 苏醒动画首帧/静态图（缺省用程序占位）
  }
}

/**
 * 默认占位文案（用户可改，manifest 未提供字段时回退）。
 *
 * 五个字段都是 **getter**：这个对象在 import 时就被建出来，写成字符串常量会把语言定死在
 * 那一刻；getter 让每一次读取都按当下这一份词条取值（与 `t()` 同一条规矩）。
 * 用户自己写的 bubble 覆盖照旧是普通字符串，它们不从词条里来。
 */
export const DEFAULT_BUBBLES: PersonaBubbles = {
  get morning() { return t('persona.bubble.morning') },
  get done() { return t('persona.bubble.done') },
  get harnessOnline() { return t('persona.bubble.harness-online') },
  get harnessOffline() { return t('persona.bubble.harness-offline') },
  get chatOpen() { return t('persona.bubble.chat-open') },
}
