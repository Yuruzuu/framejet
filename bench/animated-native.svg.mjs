export const video = { width: 96, height: 96, fps: 30, duration: 4 };

// The same four shapes/timing as tests/fixtures/animated.svg, sampled in JS.
export function render({ time }) {
  const position = Math.min(time * 32, 64);
  const fill = time >= 1 && time < 1.5 ? 'yellow' : 'purple';
  return `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96">
    <rect x="${position}" width="16" height="16" fill="red"/>
    <rect x="${position}" y="24" width="16" height="16" fill="lime"/>
    <rect x="${position}" y="48" width="16" height="16" fill="blue"/>
    <rect y="72" width="16" height="16" fill="${fill}"/>
  </svg>`;
}
