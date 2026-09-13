import { describe, expect, it, vi } from 'vitest'
import { DeepSeekWebAdapter } from '../src/chat/deepseekWebAdapter.ts'
import { nativeRuntime } from '../src/native/runtime.ts'

describe('DeepSeek web adapter', () => {
  it('uses the in-app native transport and forwards scoped events', async () => {
    const originalEnsure = nativeRuntime.ensureDeepSeekWeb
    const originalStatus = nativeRuntime.deepseekWebStatus
    const originalHistory = nativeRuntime.deepseekWebHistory
    const originalListen = nativeRuntime.listenChat
    const originalSend = nativeRuntime.sendChat
    const originalCancel = nativeRuntime.cancelChat
    let receive: ((event: Parameters<Parameters<typeof nativeRuntime.listenChat>[0]>[0]) => void) | undefined
    try {
      nativeRuntime.ensureDeepSeekWeb = vi.fn(async () => undefined)
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', model: 'deepseek-chat', signature: 'deepseek-chat-dom-v2' }))
      nativeRuntime.deepseekWebHistory = vi.fn(async () => ({ messages: [], conversationId: 'web-current', state: 'ready' as const }))
      nativeRuntime.listenChat = vi.fn(async (listener) => { receive = listener; return () => { receive = undefined } })
      nativeRuntime.sendChat = vi.fn(async (_mode, _text, options) => {
        const requestId = options?.requestId
        receive?.({ type: 'delta', text: '应用内回复', backend: 'deepseek-web', conversationId: 'web-current', requestId })
        receive?.({ type: 'message', role: 'assistant', content: '应用内回复', backend: 'deepseek-web', conversationId: 'web-current', requestId })
        receive?.({ type: 'status', activity: 'done', backend: 'deepseek-web', conversationId: 'web-current', requestId })
        // Simulate a queued native event from the just-completed turn. It
        // must not make the next render busy again.
        receive?.({ type: 'status', activity: 'sending', backend: 'deepseek-web', conversationId: 'web-current', requestId: `${requestId}-late` })
        return 'web-current'
      })
      nativeRuntime.cancelChat = vi.fn(async () => undefined)

      const adapter = new DeepSeekWebAdapter()
      const events: unknown[] = []
      adapter.subscribe((event) => events.push(event))
      await adapter.connect()
      await adapter.send('你好')

      expect(nativeRuntime.sendChat).toHaveBeenCalledWith('deepseek-web', '你好', expect.objectContaining({ conversationId: 'web-current', requestId: expect.any(String) }))
      expect(events).toEqual(expect.arrayContaining([
        { type: 'message', role: 'user', content: '你好' },
        { type: 'delta', text: '应用内回复' },
        { type: 'message', role: 'assistant', content: '应用内回复' },
        { type: 'status', activity: 'done' },
      ]))
      expect(events).not.toContainEqual({ type: 'status', activity: 'sending' })
      adapter.disconnect()
    } finally {
      nativeRuntime.ensureDeepSeekWeb = originalEnsure
      nativeRuntime.deepseekWebStatus = originalStatus
      nativeRuntime.deepseekWebHistory = originalHistory
      nativeRuntime.listenChat = originalListen
      nativeRuntime.sendChat = originalSend
      nativeRuntime.cancelChat = originalCancel
    }
  })

  it('releases the request from a final assistant message even if done is lost', async () => {
    const originalStatus = nativeRuntime.deepseekWebStatus
    const originalListen = nativeRuntime.listenChat
    const originalSend = nativeRuntime.sendChat
    let receive: ((event: Parameters<Parameters<typeof nativeRuntime.listenChat>[0]>[0]) => void) | undefined
    try {
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', signature: 'deepseek-chat-dom-v2' }))
      nativeRuntime.listenChat = vi.fn(async (listener) => { receive = listener; return () => { receive = undefined } })
      nativeRuntime.sendChat = vi.fn(async (_mode, _text, options) => {
        const requestId = options?.requestId
        receive?.({ type: 'message', role: 'assistant', content: '最终回复', backend: 'deepseek-web', conversationId: 'web-current', requestId })
        // This is intentionally after the assistant message. It represents a
        // delayed native status event that must not put the adapter back into
        // a busy state after the terminal record was delivered.
        receive?.({ type: 'status', activity: 'sending', backend: 'deepseek-web', conversationId: 'web-current', requestId: 'late' })
        return 'web-current'
      })

      const adapter = new DeepSeekWebAdapter()
      const events: unknown[] = []
      adapter.subscribe((event) => events.push(event))
      await adapter.connect()
      await adapter.send('你好')

      expect(events).toEqual(expect.arrayContaining([
        { type: 'message', role: 'assistant', content: '最终回复' },
        { type: 'status', activity: 'done' },
      ]))
      expect(events).not.toContainEqual({ type: 'status', activity: 'sending' })
      adapter.disconnect()
    } finally {
      nativeRuntime.deepseekWebStatus = originalStatus
      nativeRuntime.listenChat = originalListen
      nativeRuntime.sendChat = originalSend
    }
  })

  it('releases the request when native send resolves without a terminal event', async () => {
    const originalStatus = nativeRuntime.deepseekWebStatus
    const originalListen = nativeRuntime.listenChat
    const originalSend = nativeRuntime.sendChat
    try {
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', signature: 'deepseek-chat-dom-v2' }))
      nativeRuntime.listenChat = vi.fn(async () => () => undefined)
      nativeRuntime.sendChat = vi.fn(async () => 'web-current')

      const adapter = new DeepSeekWebAdapter()
      const events: unknown[] = []
      adapter.subscribe((event) => events.push(event))
      await adapter.connect()
      await adapter.send('你好')

      expect(events).toEqual(expect.arrayContaining([
        { type: 'message', role: 'user', content: '你好' },
        { type: 'status', activity: 'done' },
      ]))
      adapter.disconnect()
    } finally {
      nativeRuntime.deepseekWebStatus = originalStatus
      nativeRuntime.listenChat = originalListen
      nativeRuntime.sendChat = originalSend
    }
  })

  it('does not synthesize done after an error terminal event', async () => {
    const originalStatus = nativeRuntime.deepseekWebStatus
    const originalListen = nativeRuntime.listenChat
    const originalSend = nativeRuntime.sendChat
    let receive: ((event: Parameters<Parameters<typeof nativeRuntime.listenChat>[0]>[0]) => void) | undefined
    try {
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', signature: 'deepseek-chat-dom-v2' }))
      nativeRuntime.listenChat = vi.fn(async (listener) => { receive = listener; return () => { receive = undefined } })
      nativeRuntime.sendChat = vi.fn(async (_mode, _text, options) => {
        receive?.({ type: 'error', code: 'DEEPSEEK_WEB_SEND_FAILED', recoverable: true, message: '网页错误', backend: 'deepseek-web', conversationId: 'web-current', requestId: options?.requestId })
        return 'web-current'
      })

      const adapter = new DeepSeekWebAdapter()
      const events: unknown[] = []
      adapter.subscribe((event) => events.push(event))
      await adapter.connect()
      await adapter.send('你好')

      expect(events).toContainEqual({ type: 'error', code: 'DEEPSEEK_WEB_SEND_FAILED', recoverable: true, message: '网页错误' })
      expect(events).not.toContainEqual({ type: 'status', activity: 'done' })
      adapter.disconnect()
    } finally {
      nativeRuntime.deepseekWebStatus = originalStatus
      nativeRuntime.listenChat = originalListen
      nativeRuntime.sendChat = originalSend
    }
  })

  it('rejects an overlapping send before the native busy event arrives', async () => {
    const originalStatus = nativeRuntime.deepseekWebStatus
    const originalListen = nativeRuntime.listenChat
    const originalSend = nativeRuntime.sendChat
    let resolveSend: ((value: string) => void) | undefined
    try {
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', signature: 'deepseek-chat-dom-v2' }))
      nativeRuntime.listenChat = vi.fn(async () => () => undefined)
      nativeRuntime.sendChat = vi.fn(() => new Promise<string>((resolve) => { resolveSend = resolve }))

      const adapter = new DeepSeekWebAdapter()
      await adapter.connect()
      const first = adapter.send('第一条')
      await expect(adapter.send('第二条')).rejects.toThrow('上一条消息仍在处理中')
      resolveSend?.('web-current')
      await first
      adapter.disconnect()
    } finally {
      nativeRuntime.deepseekWebStatus = originalStatus
      nativeRuntime.listenChat = originalListen
      nativeRuntime.sendChat = originalSend
    }
  })

  it('releases the renderer immediately when stop acknowledgement is delayed', async () => {
    const originalStatus = nativeRuntime.deepseekWebStatus
    const originalListen = nativeRuntime.listenChat
    const originalSend = nativeRuntime.sendChat
    const originalCancel = nativeRuntime.cancelChat
    try {
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', signature: 'deepseek-chat-dom-v2' }))
      nativeRuntime.listenChat = vi.fn(async () => () => undefined)
      nativeRuntime.sendChat = vi.fn(() => new Promise<string>(() => undefined))
      nativeRuntime.cancelChat = vi.fn(async () => undefined)

      const adapter = new DeepSeekWebAdapter()
      const events: unknown[] = []
      adapter.subscribe((event) => events.push(event))
      await adapter.connect()
      void adapter.send('正在生成')
      await Promise.resolve()
      await adapter.stop()

      expect(events).toContainEqual({ type: 'status', activity: 'idle' })
      adapter.disconnect()
    } finally {
      nativeRuntime.deepseekWebStatus = originalStatus
      nativeRuntime.listenChat = originalListen
      nativeRuntime.sendChat = originalSend
      nativeRuntime.cancelChat = originalCancel
    }
  })
})
