export const video = { width: 1280, height: 720, fps: 30, duration: 4 };

const particles = Array.from({ length: 900 }, (_, index) => ({
  x: (index * 173) % 1280,
  y: (index * 97) % 720,
  radius: 1 + index % 7,
  color: ['#5cf0cb', '#6c9cff', '#f1f6ff'][index % 3],
}));

export function render({ time }) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720">
    <rect width="1280" height="720" fill="#101827"/>
    ${particles.map((particle, index) => `<circle cx="${(particle.x + time * (8 + index % 20)) % 1280}" cy="${particle.y + Math.sin(time + index) * 12}" r="${particle.radius}" fill="${particle.color}" opacity="0.7"/>`).join('')}
  </svg>`;
}
