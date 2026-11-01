import { setup as setupBands } from './bands.three.mjs';

export const video = { width: 64, height: 64, fps: 4, duration: 5 };

export async function setup(context) {
  const state = await setupBands(context);
  const update = state.update;
  state.update = sample => {
    if (sample.frame === 18) throw new Error('smoke failure');
    update(sample);
  };
  return state;
}
