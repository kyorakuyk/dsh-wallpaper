/**
 * 中文词条 —— **全量真源**（两份合起来）。
 *
 * 词条按"面"分成两半，这里只负责合并，别在这个文件里写词条：
 * 1. `zh.shared.ts` —— 完整版与 Lite 都要说的句子（Lite **只有**这一半）；
 * 2. `zh.full.ts` —— 只有完整版会说的句子。
 *
 * 为什么拆开：整本词典曾经由 `i18n/index.ts` 直接持有，而 Lite 入口也 import 它，于是完整版的每一条
 * 键名与文案都进了 `dist-lite` 的产物（`scripts/verify-lite-bundle.ps1` 就是拦这件事的那道关）。
 * 现在机制（`i18n/index.ts`）不认识任何词典，两个入口各自登记自己那份：`i18n/full.ts` 交这里，
 * `i18n/lite.ts` 只交 `zh.shared.ts`。边界由 `tests/liteI18nBoundary.spec.ts` 钉住。
 *
 * 规则（拆分前就是这样，拆分没有改动任何一条文案）：
 * 1. 中文是**从界面上原样搬过来的**，不是重写的 —— 所以"中文界面没有变化"是可验证的；
 * 2. 键按界面位置命名（`settings.general.title`、`language.label`），不按句意命名；
 * 3. 新增一个键，`en.ts` 不同步就会**编译不过**（`Dict` 由这里推导）；
 * 4. 需要插值的地方用 `{name}`，由 `t(key, { name })` 填。
 */
import { zhFull } from './zh.full.ts'
import { zhShared, type SharedDict } from './zh.shared.ts'

export const zh = { ...zhShared, ...zhFull }

/** 所有可用键。写错键名在编译期就会被挡住。 */
export type MessageKey = keyof typeof zh

/**
 * 一份完整词条的形状。
 *
 * `en.ts` 用它标注，于是"少一个键"或"多一个键"都会在 `pnpm typecheck` 里失败 ——
 * 比"跑测试才发现漏了"更早，而且不需要为完整性写测试。
 */
export type Dict = Record<MessageKey, string>

/**
 * 断言 `Sub` 的键都在 `Super` 里：不满足时是**编译错误**，不是运行时才发现。
 *
 * 键集合之间的关系（Lite ⊆ 全量）本来靠 `zh.ts` 的结构成立，写成显式的类型锁是为了让它
 * 在类型层面看得见、也拦得住重构 —— 例如哪天这个文件忘了把 `zhShared` 合进来。
 */
export type AssertKeysIn<Sub extends Super, Super> = Sub extends Super ? true : never

/** Lite 那一半（`zh.shared.ts`）的键必须都在全量键集合里 —— "Lite ⊆ 全量"的类型锁。 */
export type SharedKeysAreFullKeys = AssertKeysIn<keyof SharedDict, MessageKey>
