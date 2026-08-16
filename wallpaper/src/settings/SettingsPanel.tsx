import type { BackendMode, ModelTierRule } from '../domain/types.ts'
import { BACKGROUND_OPTIONS, type WallpaperSettings } from './store.ts'

export interface SettingsPanelProps {
  settings: WallpaperSettings
  harnessStatus: 'offline' | 'web-only' | 'bridge-ready'
  onChange: (settings: WallpaperSettings) => void
  onRequestDeepSeekLogin: () => void
  onConfigureApiKey: () => void
  onClose: () => void
}

export function SettingsPanel({ settings, harnessStatus, onChange, onRequestDeepSeekLogin, onConfigureApiKey, onClose }: SettingsPanelProps) {
  const set = (patch: Partial<WallpaperSettings>) => onChange({ ...settings, ...patch })
  const addRule = () => set({ modelTierRules: [...settings.modelTierRules, { backend: '*', pattern: '', match: 'contains', tier: 'flash' }] })
  const updateRule = (index: number, patch: Partial<ModelTierRule>) => set({ modelTierRules: settings.modelTierRules.map((rule, i) => i === index ? { ...rule, ...patch } : rule) })
  return (
    <div className="settings-panel">
      <div className="settings-header"><span>⚙️ 壁纸设置</span><button className="settings-close" onClick={onClose}>✕</button></div>
      <div className="settings-section">
        <h4>后端与会话</h4>
        <label className="setting-row">默认模式：<select value={settings.defaultBackend} onChange={(event) => set({ defaultBackend: event.target.value as BackendMode })}><option value="deepseek-web">DeepSeek 免费网页桥接</option><option value="deepseek-api">DeepSeek API（计费）</option><option value="harness">DeepSeek Harness</option></select></label>
        <label className="setting-row">会话策略：<select value={settings.conversationPolicy} onChange={(event) => set({ conversationPolicy: event.target.value as WallpaperSettings['conversationPolicy'] })}><option value="resume-last">恢复最近会话</option><option value="new-on-unlock">每次解锁新建</option><option value="daily">每日新建</option></select></label>
        <label className="setting-row"><input type="checkbox" checked={settings.autoSwitchHarness} onChange={(event) => set({ autoSwitchHarness: event.target.checked })} />DSH bridge 就绪时自动切换</label>
        <div className={`conn-state ${harnessStatus === 'bridge-ready' ? 'on' : 'off'}`}>{harnessStatus === 'bridge-ready' ? '● DSH 壁纸桥接已就绪' : harnessStatus === 'web-only' ? '◐ DSH 在线，但未安装壁纸桥接' : '○ DSH 离线'}</div>
      </div>
      <div className="settings-section">
        <h4>DeepSeek 连接</h4>
        <button className="setting-btn" onClick={onRequestDeepSeekLogin}>扫码登录 / 重新登录网页桥接</button>
        <label className="setting-row">API 地址：<input type="text" value={settings.deepseekApi.baseUrl} onChange={(event) => set({ deepseekApi: { ...settings.deepseekApi, baseUrl: event.target.value } })} /></label>
        <label className="setting-row">API 模型：<input type="text" value={settings.deepseekApi.model} onChange={(event) => set({ deepseekApi: { ...settings.deepseekApi, model: event.target.value } })} /></label>
        <button className="setting-btn secondary" onClick={onConfigureApiKey}>在 Windows 凭据管理器中设置 API Key</button>
        <div className="settings-hint">API 模式会产生实际费用；网页桥接故障时不会自动转为 API。</div>
      </div>
      <div className="settings-section">
        <h4>苏醒与系统</h4>
        <label className="setting-row"><input type="checkbox" checked={settings.animationsEnabled} onChange={(event) => set({ animationsEnabled: event.target.checked })} />启用苏醒动画</label>
        <label className="setting-row"><input type="checkbox" checked={settings.playWakeOnEveryUnlock} onChange={(event) => set({ playWakeOnEveryUnlock: event.target.checked })} />每次解锁播放</label>
        <label className="setting-row"><input type="checkbox" checked={settings.skipWakeAnimation} onChange={(event) => set({ skipWakeAnimation: event.target.checked })} />跳过动画</label>
        <label className="setting-row">速度：<input type="range" min="0.5" max="2" step="0.1" value={settings.animationSpeed} onChange={(event) => set({ animationSpeed: Number(event.target.value) })} />{settings.animationSpeed}×</label>
        <label className="setting-row"><input type="checkbox" checked={settings.lockScreenEnabled} onChange={(event) => set({ lockScreenEnabled: event.target.checked })} />用熟睡图接管当前用户锁屏</label>
        <label className="setting-row"><input type="checkbox" checked={settings.autostart} onChange={(event) => set({ autostart: event.target.checked })} />登录后自动启动</label>
      </div>
      <div className="settings-section">
        <h4>模型 → 形态规则</h4>
        {settings.modelTierRules.map((rule, index) => <div className="model-rule" key={`${index}-${rule.pattern}`}><select value={rule.backend} onChange={(event) => updateRule(index, { backend: event.target.value as ModelTierRule['backend'] })}><option value="*">所有后端</option><option value="deepseek-web">DeepSeek Web</option><option value="deepseek-api">DeepSeek API</option><option value="harness">Harness</option></select><select value={rule.match} onChange={(event) => updateRule(index, { match: event.target.value as ModelTierRule['match'] })}><option value="exact">精确</option><option value="contains">包含</option><option value="regex">正则</option></select><input value={rule.pattern} placeholder="模型名称" onChange={(event) => updateRule(index, { pattern: event.target.value })} /><select value={rule.tier} onChange={(event) => updateRule(index, { tier: event.target.value as ModelTierRule['tier'] })}><option value="flash">Flash / 幼年</option><option value="pro">Pro / 成年</option></select><button onClick={() => set({ modelTierRules: settings.modelTierRules.filter((_, i) => i !== index) })}>删</button></div>)}
        <button className="setting-btn secondary" onClick={addRule}>新增别名规则</button>
      </div>
      <div className="settings-section"><h4>画面</h4><div className="persona-grid">{BACKGROUND_OPTIONS.map((background) => <button key={background.id} className={`persona-option ${settings.background === background.id ? 'active' : ''}`} onClick={() => set({ background: background.id })}>{background.name}</button>)}</div></div>
      <div className="settings-footer">dsh-wallpaper v0.2.0 · Windows 11 单主屏</div>
    </div>
  )
}

