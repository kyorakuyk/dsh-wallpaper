/** 程序化占位鲸鱼娘：Canvas 绘制立绘（睡眠/苏醒/待机姿态），素材就绪前保证全流程可跑 */

export type WhalePose = 'sleep' | 'wake' | 'idle'

export interface WhalePalette {
  hair: string
  hairDark: string
  skin: string
  dress: string
  fin: string // 鱼尾/鳍
  accent: string
}

export const PALETTES = {
  blue: { hair: '#3f8fdd', hairDark: '#2c6cb0', skin: '#ffe9dc', dress: '#e8f4ff', fin: '#4da6ff', accent: '#ffd9e8' },
  black: { hair: '#2b2f3a', hairDark: '#1a1d26', skin: '#ffe9dc', dress: '#3a3f4d', fin: '#e03050', accent: '#ff6b81' },
} as const

/** 绘制立绘到 canvas；返回 dataURL（透明底 PNG），供 <img> 使用 */
export function drawWhale(
  palette: WhalePalette,
  pose: WhalePose,
  size = 360,
): string {
  const canvas = document.createElement('canvas')
  canvas.width = size
  canvas.height = size
  const ctx = canvas.getContext('2d')
  if (!ctx) return ''
  ctx.clearRect(0, 0, size, size)

  const cx = size / 2
  const cy = size / 2
  const s = size / 360 // 缩放基准

  // 鱼尾（底部，随姿态摆动）
  ctx.fillStyle = palette.fin
  ctx.beginPath()
  ctx.moveTo(cx - 30 * s, cy + 60 * s)
  ctx.quadraticCurveTo(cx - 90 * s, cy + 20 * s, cx - 60 * s, cy + 90 * s)
  ctx.quadraticCurveTo(cx - 30 * s, cy + 80 * s, cx, cy + 75 * s)
  ctx.quadraticCurveTo(cx + 30 * s, cy + 80 * s, cx + 60 * s, cy + 90 * s)
  ctx.quadraticCurveTo(cx + 90 * s, cy + 20 * s, cx + 30 * s, cy + 60 * s)
  ctx.closePath()
  ctx.fill()

  // 连衣裙（身体）
  ctx.fillStyle = palette.dress
  ctx.beginPath()
  ctx.moveTo(cx - 42 * s, cy - 20 * s)
  ctx.quadraticCurveTo(cx - 70 * s, cy + 40 * s, cx - 46 * s, cy + 66 * s)
  ctx.lineTo(cx + 46 * s, cy + 66 * s)
  ctx.quadraticCurveTo(cx + 70 * s, cy + 40 * s, cx + 42 * s, cy - 20 * s)
  ctx.closePath()
  ctx.fill()
  ctx.strokeStyle = palette.accent
  ctx.lineWidth = 3 * s
  ctx.stroke()

  // 头
  ctx.fillStyle = palette.skin
  ctx.beginPath()
  ctx.arc(cx, cy - 48 * s, 42 * s, 0, Math.PI * 2)
  ctx.fill()

  // 刘海
  ctx.fillStyle = palette.hair
  ctx.beginPath()
  ctx.arc(cx, cy - 56 * s, 44 * s, Math.PI, 0)
  ctx.fill()
  ctx.beginPath()
  ctx.moveTo(cx - 40 * s, cy - 52 * s)
  ctx.quadraticCurveTo(cx - 22 * s, cy - 30 * s, cx - 4 * s, cy - 48 * s)
  ctx.quadraticCurveTo(cx + 10 * s, cy - 26 * s, cx + 34 * s, cy - 50 * s)
  ctx.quadraticCurveTo(cx + 12 * s, cy - 64 * s, cx - 8 * s, cy - 62 * s)
  ctx.closePath()
  ctx.fill()

  // 两侧长发
  ctx.beginPath()
  ctx.moveTo(cx - 42 * s, cy - 40 * s)
  ctx.quadraticCurveTo(cx - 66 * s, cy - 8 * s, cx - 52 * s, cy + 24 * s)
  ctx.quadraticCurveTo(cx - 44 * s, cy + 10 * s, cx - 38 * s, cy + 4 * s)
  ctx.closePath()
  ctx.fill()
  ctx.beginPath()
  ctx.moveTo(cx + 42 * s, cy - 40 * s)
  ctx.quadraticCurveTo(cx + 66 * s, cy - 8 * s, cx + 52 * s, cy + 24 * s)
  ctx.quadraticCurveTo(cx + 44 * s, cy + 10 * s, cx + 38 * s, cy + 4 * s)
  ctx.closePath()
  ctx.fill()

  // 眼睛（睡眠=闭眼线，其余=豆豆眼）
  ctx.strokeStyle = '#3a2a20'
  ctx.lineWidth = 3 * s
  if (pose === 'sleep') {
    ctx.beginPath()
    ctx.moveTo(cx - 18 * s, cy - 50 * s)
    ctx.quadraticCurveTo(cx - 12 * s, cy - 46 * s, cx - 6 * s, cy - 50 * s)
    ctx.stroke()
    ctx.beginPath()
    ctx.moveTo(cx + 6 * s, cy - 50 * s)
    ctx.quadraticCurveTo(cx + 12 * s, cy - 46 * s, cx + 18 * s, cy - 50 * s)
    ctx.stroke()
  } else {
    ctx.fillStyle = '#3a2a20'
    ctx.beginPath()
    ctx.arc(cx - 14 * s, cy - 50 * s, 4.5 * s, 0, Math.PI * 2)
    ctx.fill()
    ctx.beginPath()
    ctx.arc(cx + 14 * s, cy - 50 * s, 4.5 * s, 0, Math.PI * 2)
    ctx.fill()
  }

  // 腮红
  ctx.fillStyle = 'rgba(255,140,160,0.45)'
  ctx.beginPath()
  ctx.ellipse(cx - 30 * s, cy - 40 * s, 7 * s, 4 * s, 0, 0, Math.PI * 2)
  ctx.fill()
  ctx.beginPath()
  ctx.ellipse(cx + 30 * s, cy - 40 * s, 7 * s, 4 * s, 0, 0, Math.PI * 2)
  ctx.fill()

  // 鲸鱼耳鳍（头顶小鳍）
  ctx.fillStyle = palette.fin
  ctx.beginPath()
  ctx.moveTo(cx - 30 * s, cy - 88 * s)
  ctx.quadraticCurveTo(cx - 14 * s, cy - 108 * s, cx - 2 * s, cy - 92 * s)
  ctx.closePath()
  ctx.fill()

  // 睡眠态：画 Zzz
  if (pose === 'sleep') {
    ctx.fillStyle = palette.hairDark
    ctx.font = `${Math.round(22 * s)}px sans-serif`
    ctx.fillText('Z', cx + 46 * s, cy - 74 * s)
    ctx.fillText('z', cx + 62 * s, cy - 58 * s)
    ctx.fillText('z', cx + 74 * s, cy - 44 * s)
  }

  // 苏醒态：画"起"字气泡角标（简易表示刚醒）
  if (pose === 'wake') {
    ctx.strokeStyle = palette.accent
    ctx.lineWidth = 4 * s
    ctx.beginPath()
    ctx.moveTo(cx - 46 * s, cy + 8 * s)
    ctx.lineTo(cx - 46 * s, cy + 34 * s)
    ctx.stroke()
  }

  return canvas.toDataURL('image/png')
}

/** 生成形态立绘 URL（程序占位）。素材就绪后由 persona 的 assets.portrait 覆盖。 */
export function placeholderPortrait(kind: 'blue' | 'black', pose: WhalePose): string {
  return drawWhale(PALETTES[kind], pose)
}
