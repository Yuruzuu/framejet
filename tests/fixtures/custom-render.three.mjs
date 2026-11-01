import { setup as setupBands } from './bands.three.mjs';
export { video } from './metadata.mjs';

export async function setup(context) {
  const state = await setupBands(context);
  return {
    ...state,
    render({ renderer }) { renderer.render(state.scene, state.camera); },
  };
}
