/**
 * 单测跑在**完整版**的词典上。
 *
 * 词条现在由入口登记（`src/main.tsx` 登记 `i18n/full.ts`，`src/main-lite.tsx` 登记 `i18n/lite.ts`），
 * 机制（`i18n/index.ts`）自己不认识任何词典。测试没有入口，所以在这里登记一次 —— 与 `main.tsx`
 * 那一行是同一件事。不装这个文件，`t()` 就只剩机制：每条断言都会看到键名，而不是句子。
 *
 * Lite 那一半的边界不靠这里，靠 `tests/liteI18nBoundary.spec.ts`（它读的是 `LITE_DICTIONARIES`，
 * 与这里的登记互不干扰）。
 */
import '../src/i18n/full.ts'
