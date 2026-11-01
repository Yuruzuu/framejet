export const video = { width: 960, height: 540, fps: 30, duration: 4 };

export function render({ time, width, height, duration }) {
  const progress = time / duration;
  const angle = progress * Math.PI * 2;
  const orbitX = 710 + Math.cos(angle) * 95;
  const orbitY = 240 + Math.sin(angle) * 95;
  const reveal = Math.min(1, time / 0.8);
  const titleY = 244 + (1 - reveal) ** 3 * 35;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 960 540">
    <defs>
      <linearGradient id="background" x2="1" y2="1">
        <stop stop-color="#101b35"/><stop offset="1" stop-color="#080d19"/>
      </linearGradient>
      <linearGradient id="accent"><stop stop-color="#5cf0cb"/><stop offset="1" stop-color="#6c9cff"/></linearGradient>
    </defs>
    <rect width="960" height="540" fill="url(#background)"/>
    <circle cx="710" cy="240" r="140" fill="none" stroke="#203254" stroke-width="1"/>
    <circle cx="710" cy="240" r="95" fill="none" stroke="#304b72" stroke-width="2"/>
    <circle cx="710" cy="240" r="52" fill="url(#accent)"/>
    <circle cx="${orbitX}" cy="${orbitY}" r="12" fill="#d6fff4"/>
    <g opacity="${reveal}" font-family="Arial, sans-serif">
      <text x="76" y="${titleY}" font-size="76" font-weight="bold" fill="#f1f6ff">Framejet</text>
      <text x="80" y="296" font-size="23" fill="#a7bad9">From an idea to every frame.</text>
    </g>
    <rect x="80" y="420" width="800" height="4" rx="2" fill="#203254"/>
    <rect x="80" y="420" width="${800 * progress}" height="4" rx="2" fill="url(#accent)"/>
    <text x="80" y="464" font-family="Arial, sans-serif" font-size="14" fill="#7d94b5">SVG · NATIVE RENDERING · ${time.toFixed(2)}s</text>
  </svg>`;
}
