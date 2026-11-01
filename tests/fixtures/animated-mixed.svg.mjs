import { render as animated } from './animated-module.svg.mjs';
export { video } from './animated-module.svg.mjs';

export function render({ time }) {
  if (time < 0.5) return '<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="16" height="16" fill="red"/></svg>';
  return animated();
}
