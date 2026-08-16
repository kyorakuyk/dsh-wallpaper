import type { BackendMode, ChatEvent, ChatMessage } from '../domain/types.ts'

export interface SendOptions {
  conversationId?: string
  model?: string
}

export interface ChatAdapter {
  readonly mode: BackendMode
  connect(): Promise<void>
  disconnect(): void
  send(text: string, options?: SendOptions): Promise<void>
  stop(): Promise<void>
  history(conversationId?: string): Promise<ChatMessage[]>
  subscribe(listener: (event: ChatEvent) => void): () => void
}

export abstract class EventChatAdapter implements ChatAdapter {
  abstract readonly mode: BackendMode
  private listeners = new Set<(event: ChatEvent) => void>()
  abstract connect(): Promise<void>
  abstract disconnect(): void
  abstract send(text: string, options?: SendOptions): Promise<void>
  abstract stop(): Promise<void>
  abstract history(conversationId?: string): Promise<ChatMessage[]>
  subscribe(listener: (event: ChatEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }
  protected emit(event: ChatEvent): void {
    for (const listener of this.listeners) listener(event)
  }
}

