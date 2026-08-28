/** Resolve a packaged/public asset without coupling small entries to the
 * full settings or conversation store. */
export function assetUrl(path: string): string {
  return `${import.meta.env.BASE_URL}${path.replace(/^\//, '')}`
}
