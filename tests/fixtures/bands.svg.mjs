export const video = { width: 64, height: 64, fps: 4, duration: 1 };

export function render({ time }) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
    <rect width="64" height="32" fill="red"/>
    <rect y="32" width="64" height="32" fill="blue"/>
    <rect x="${3 + time * 48}" y="29" width="6" height="6" fill="white"/>
  </svg>`;
}
