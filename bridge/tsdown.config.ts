export default {
  name: 'dsh-wallpaper-bridge',
  entry: { index: 'src/index.ts', protocol: 'src/protocol.ts' },
  outDir: 'lib',
  format: ['esm'],
  platform: 'node',
  target: 'es2022',
  fixedExtension: false,
  dts: false,
  clean: true,
}
