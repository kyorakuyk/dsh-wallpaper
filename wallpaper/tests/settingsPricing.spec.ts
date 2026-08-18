import { describe, expect, it } from 'vitest'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { MAX_PRICE_PER_MILLION, normalizedPrice } from '../src/settings/store.ts'
import { PriceInput } from '../src/settings/SettingsPanel.tsx'

describe('API pricing setting normalization', () => {
  it('keeps valid zero pricing but rejects malformed or unsafe rates', () => {
    expect(normalizedPrice(0)).toBe(0)
    expect(normalizedPrice(MAX_PRICE_PER_MILLION)).toBe(MAX_PRICE_PER_MILLION)
    expect(normalizedPrice(MAX_PRICE_PER_MILLION + 0.01)).toBeUndefined()
    expect(normalizedPrice(-1)).toBeUndefined()
    expect(normalizedPrice(Infinity)).toBeUndefined()
    expect(normalizedPrice('1')).toBeUndefined()
  })

  it('exposes the native price ceiling in the settings input', () => {
    const html = renderToStaticMarkup(createElement(PriceInput, {
      label: '输入价格',
      value: undefined,
      onChange: () => undefined,
    }))

    expect(html).toContain(`max="${MAX_PRICE_PER_MILLION}"`)
  })
})
