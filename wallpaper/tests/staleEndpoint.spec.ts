import { describe, expect, it } from 'vitest'
import { staleEndpointPort } from '../src/connect/endpoints.ts'

/**
 * 设置里那条"手工指定端口"与当前主体是否自相矛盾。
 *
 * 这个函数是**存量那一侧唯一的守门人**：它判定的正是"用户当年为另一个主体选的端口，换了主体之后
 * 还在生效"。实测事故：主体是只该用 3080 的已安装 CLI，pin 还停在官方客户端的 19387，于是点
 * 「打开」把官方客户端的窗口拉到了前台。四条分支都钉在这里，因为它纯、便宜，而且已经漏过一次。
 */
describe('staleEndpointPort', () => {
  it('reports nothing when no port is pinned', () => {
    expect(staleEndpointPort({ subjectId: 'cli:C:/npm/dsh.cmd' })).toBeUndefined()
  })

  it('reports nothing when the pin has no subject to belong to', () => {
    // 没有主体时这条 pin 无所属 —— 它不是矛盾，只是暂时没人用。
    expect(staleEndpointPort({ endpointPort: 19387 })).toBeUndefined()
  })

  it('accepts a pin inside the subject own ports', () => {
    // 已安装的 CLI 自己的端口就是 3080（`subjectEndpointPorts` 实测），pin 在这里面不算矛盾。
    expect(staleEndpointPort({ subjectId: 'cli:C:/npm/dsh.cmd', endpointPort: 3080 })).toBeUndefined()
  })

  it('reports a pin that belongs to another subject, and changes nothing', () => {
    expect(staleEndpointPort({ subjectId: 'cli:C:/npm/dsh.cmd', endpointPort: 19387 })).toBe(19387)
    // 反过来也成立：官壳那一类自己的端口是 19387，指着 3080 同样是矛盾。
    expect(staleEndpointPort({ subjectId: 'shell:com.deepseek.dsh', endpointPort: 3080 })).toBe(3080)
  })
})
