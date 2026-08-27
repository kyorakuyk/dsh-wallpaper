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
      nativeRuntime.deepseekWebStatus = vi.fn(async () => ({ state: 'ready', conversationId: 'web-current', model: 'deepseek-chat', signature: 'deepseek-chat-dom-v1' }))
      nativeRuntime.deepseekWebHistory = vi.fn(async () => ({ messages: [], conversationId: 'web-current', state: 'ready' as const }))
      nativeRuntime.listenChat = vi.fn(async (listener) => { receive = listener; return () => { receive = undefined } })
      nativeRuntime.sendChat = vi.fn(async (_mode, _text, options) => {
        const requestId = options?.requestId
        receive?.({ type: 'delta', text: '应用内回复', backend: 'deepseek-web', conversationId: 'web-current' })
        receive?.({ type: 'message', role: 'assistant', content: '应用内回复', backend: 'deepseek-web', conversationId: 'web-current' })
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
})
