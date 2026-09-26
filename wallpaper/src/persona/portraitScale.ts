/**
 * 立绘的体型适配系数。
 *
 * 幼态与成年态是同一个角色的两套立绘：槽位高度一样（`.portrait-slot` 300×420，`object-fit:
 * contain`），但两套画的身体比例不同——幼态是大头短身，成年态是长腿细腰。同高渲染之下，
 * 成年态的头明显偏小，于是两个形态看起来不像同一个角色（用户实测报的就是这条）。
 *
 * 实测（`artifacts/persona-head.py`，按**渲染后**的像素量）：
 *   黑红那对：同高之下幼态头高 151px、成年态 138px → 成年/幼态 = 0.914
 *   蓝色那对：成年态的头被长发与裙摆挡住，脖子探不出来，量不出可靠数字，**视觉一致**
 * 所以成年态整体等比放大 9%：按 138 × 1.09 ≈ 150 ≈ 151，头一样大，而个子变高——正好是
 * "同一个角色长大了一岁"该有的样子。
 *
 * 只对**内置的官方成年立绘**生效。用户导入的 JPG 立绘走 `.portrait-asset`（`object-fit:
 * cover`、圆角、底部淡出），构图由用户自己定，不该被这个系数改。
 */
export const PORTRAIT_ADULT_SCALE = 1.09

const OFFICIAL_ADULT_PORTRAITS = new Set(['blue-adult', 'black-adult'])

export function portraitAgeScale(persona: { id: string }): number {
  return OFFICIAL_ADULT_PORTRAITS.has(persona.id) ? PORTRAIT_ADULT_SCALE : 1
}
