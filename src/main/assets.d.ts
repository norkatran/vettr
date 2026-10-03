/** electron-vite resolves `?asset` imports to a file path that works in dev and in the built app. */
declare module '*?asset' {
  const path: string
  export default path
}
