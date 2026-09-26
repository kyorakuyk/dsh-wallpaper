const FRAME_DECODE_TIMEOUT_MS = 1200
const RELEASE_ATTEMPTS = 4

export interface NativeBootstrapControl {
  releaseNativeBootstrap(generation: number): Promise<boolean>
  nativeBootstrapGeneration(): Promise<number>
}

export function decodeImageSource(src: string, timeoutMs = FRAME_DECODE_TIMEOUT_MS): Promise<boolean> {
  return new Promise((resolve) => {
    const image = new Image()
    let settled = false
    const finish = (ready: boolean) => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      image.onload = null
      image.onerror = null
      resolve(ready)
    }
    const decode = () => {
      if (typeof image.decode !== 'function') {
        finish(image.naturalWidth > 0)
        return
      }
      void image.decode().then(
        () => finish(image.naturalWidth > 0),
        () => finish(false),
      )
    }
    const timeout = setTimeout(() => finish(false), timeoutMs)
    image.onload = decode
    image.onerror = () => finish(false)
    image.src = src
    if (image.complete) decode()
  })
}

function decodeImageElement(image: HTMLImageElement, timeoutMs = FRAME_DECODE_TIMEOUT_MS): Promise<boolean> {
  return new Promise((resolve) => {
    let settled = false
    const finish = (ready: boolean) => {
      if (settled) return
      settled = true
      window.clearTimeout(timeout)
      image.removeEventListener('load', onLoad)
      image.removeEventListener('error', onError)
      resolve(ready)
    }
    const decode = () => {
      if (image.naturalWidth <= 0) {
        finish(false)
        return
      }
      if (typeof image.decode !== 'function') {
        finish(true)
        return
      }
      void image.decode().then(() => finish(image.naturalWidth > 0), () => finish(false))
    }
    const onLoad = () => decode()
    const onError = () => finish(false)
    const timeout = window.setTimeout(() => finish(false), timeoutMs)
    if (image.complete) decode()
    else {
      image.addEventListener('load', onLoad, { once: true })
      image.addEventListener('error', onError, { once: true })
    }
  })
}

export async function decodeSceneImages(root: ParentNode | null = document.querySelector('.wallpaper-root')): Promise<boolean> {
  const scene = root?.querySelector('.scene')
  if (!scene) return false
  const images = [...scene.querySelectorAll('img')].filter((image) => {
    const style = window.getComputedStyle(image)
    return style.display !== 'none' && style.visibility !== 'hidden'
  })
  if (images.length === 0) return true
  const results = await Promise.all(images.map((image) => decodeImageElement(image)))
  return results.every(Boolean)
}

function nextAnimationFrame(signal?: AbortSignal): Promise<boolean> {
  if (signal?.aborted) return Promise.resolve(false)
  return new Promise((resolve) => {
    let settled = false
    const finish = (ready: boolean) => {
      if (settled) return
      settled = true
      signal?.removeEventListener('abort', onAbort)
      resolve(ready)
    }
    let id = 0
    const onAbort = () => {
      window.cancelAnimationFrame(id)
      finish(false)
    }
    id = window.requestAnimationFrame(() => finish(!signal?.aborted))
    signal?.addEventListener('abort', onAbort, { once: true })
  })
}

export async function releaseAfterPreparedFrame(
  generation: number,
  release: (generation: number) => Promise<boolean>,
  currentGeneration: () => Promise<number>,
  signal?: AbortSignal,
  requestFrame: (signal?: AbortSignal) => Promise<boolean> = nextAnimationFrame,
): Promise<boolean> {
  try {
    for (let attempt = 0; attempt < RELEASE_ATTEMPTS; attempt += 1) {
      if (!await requestFrame(signal) || !await requestFrame(signal)) return false
      if (signal?.aborted) return false
      if (await release(generation)) return true
      if (await currentGeneration() !== generation) return false
    }
  } catch (error) {
    console.warn('native hand-off request failed', error)
  }
  return false
}

export async function reportNativeBootstrapReady(
  generation: number,
  control: NativeBootstrapControl,
  options: { signal?: AbortSignal; verifySceneImages?: boolean } = {},
): Promise<boolean> {
  if (options.signal?.aborted) return false
  if (options.verifySceneImages !== false && !await decodeSceneImages()) return false
  return releaseAfterPreparedFrame(
    generation,
    (current) => control.releaseNativeBootstrap(current),
    () => control.nativeBootstrapGeneration(),
    options.signal,
  )
}
