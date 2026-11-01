import { readFile, realpath } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';

export async function createSvgBrowser(configuration, video) {
  if (configuration.nativeFonts)
    throw new Error('Browser SVG uses system fonts and CSS @font-face; --font is native-only');
  let chromium;
  try {
    ({ chromium } = await import('playwright-core'));
  } catch {
    throw new Error('CSS/SMIL rendering requires Chromium. Run: framejet setup');
  }

  const root = await realpath(path.dirname(configuration.input));
  const scenePath = `/project/__framejet_${process.pid}.svg`;
  const mime = {
    '.svg': 'image/svg+xml',
    '.css': 'text/css',
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.webp': 'image/webp',
    '.gif': 'image/gif',
    '.woff': 'font/woff',
    '.woff2': 'font/woff2',
    '.ttf': 'font/ttf',
    '.otf': 'font/otf',
  };
  let markup = '';
  let loaded = false;
  let failures = [];
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://localhost');
      response.setHeader('Cache-Control', 'no-store');
      if (url.pathname === '/') {
        response.setHeader('Content-Type', 'text/html');
        response.end(
          '<!doctype html><link rel="icon" href="data:,"><style>html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#000}iframe{position:absolute;left:0;top:0;border:0;transform-origin:0 0}</style><iframe scrolling="no"></iframe>',
        );
      } else if (url.pathname === scenePath) {
        response.setHeader('Content-Type', 'image/svg+xml');
        response.end(markup);
      } else if (url.pathname.startsWith('/project/')) {
        const file = await realpath(
          path.resolve(root, decodeURIComponent(url.pathname.slice('/project/'.length))),
        );
        if (!file.startsWith(`${root}${path.sep}`)) {
          response.writeHead(403).end();
          return;
        }
        response.setHeader(
          'Content-Type',
          mime[path.extname(file).toLowerCase()] || 'application/octet-stream',
        );
        response.end(await readFile(file));
      } else response.writeHead(404).end();
    } catch {
      response.writeHead(404).end();
    }
  });
  let browser;
  const close = async () => {
    await browser?.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  };
  try {
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    try {
      browser = await chromium.launch({ headless: true });
    } catch (error) {
      throw new Error(`Cannot launch the SVG renderer. Run framejet setup. ${error.message}`);
    }
    const page = await browser.newPage({
      viewport: { width: video.width, height: video.height },
      deviceScaleFactor: 1,
      javaScriptEnabled: false,
      reducedMotion: 'no-preference',
    });
    page.on('requestfailed', (request) =>
      failures.push(`${request.url()}: ${request.failure()?.errorText}`),
    );
    page.on('response', (response) => {
      if (response.status() >= 400) failures.push(`${response.status()} ${response.url()}`);
    });
    const origin = `http://127.0.0.1:${server.address().port}`;
    await page.goto(origin);
    const element = await page.$('iframe');
    const frame = await element.contentFrame();

    const prepare = async (source) => {
      markup = source;
      failures = [];
      await element.evaluate((iframe, size) => {
        iframe.style.width = `${size.width}px`;
        iframe.style.height = `${size.height}px`;
        iframe.style.transform = 'none';
      }, configuration.svgSize || video);
      await frame.goto(`${origin}${scenePath}`);
      const size = await frame.evaluate(() => {
        const svg = document.documentElement;
        if (!(svg instanceof SVGSVGElement))
          throw new Error('Invalid SVG XML: expected an SVG document');
        if (svg.querySelector('script, foreignObject'))
          throw new Error('SVG scripts and foreignObject are unsupported');
        svg.pauseAnimations();
        svg.setCurrentTime(0);
        return {
          width: svg.width.baseVal.value || svg.viewBox.baseVal.width || 300,
          height: svg.height.baseVal.value || svg.viewBox.baseVal.height || 150,
        };
      });
      const scale = Math.min(video.width / size.width, video.height / size.height);
      await element.evaluate(
        (iframe, layout) => {
          iframe.style.width = `${layout.width}px`;
          iframe.style.height = `${layout.height}px`;
          iframe.style.transform = `translate(${layout.left}px, ${layout.top}px) scale(${layout.scale})`;
        },
        {
          ...size,
          scale,
          left: (video.width - size.width * scale) / 2,
          top: (video.height - size.height * scale) / 2,
        },
      );
      await frame.evaluate(async () => {
        const svg = document.documentElement;
        svg.pauseAnimations();
        svg.setCurrentTime(0);
        // Restart CSS timelines after resources load, even when a short animation
        // would have finished during navigation. Pause before sampling any frame.
        const reset = document.createElementNS('http://www.w3.org/2000/svg', 'style');
        reset.textContent =
          '* { animation-name: none !important; transition-property: none !important; }';
        svg.append(reset);
        document.getAnimations();
        reset.remove();
        window.framejetSvg = document.getAnimations().map((animation) => {
          if (!(animation.timeline instanceof DocumentTimeline))
            throw new Error('SVG export requires time-based CSS animations');
          const paused = animation.playState === 'paused';
          // CSS-owned animations can finish or be replaced during style updates.
          // A controlled copy retains the keyframes/timing without that clock.
          const controlled = new Animation(new KeyframeEffect(animation.effect), document.timeline);
          animation.cancel();
          controlled.pause();
          controlled.currentTime = 0;
          return { animation: controlled, paused };
        });
        await Promise.all(window.framejetSvg.map(({ animation }) => animation.ready));
        await document.fonts.ready;
      });
      if (failures.length) throw new Error(`SVG resources failed to load: ${failures.join('; ')}`);
      loaded = true;
    };

    return {
      async render(source, time, reuse = false) {
        if (!loaded || !reuse) await prepare(source);
        await frame.evaluate((time) => {
          for (const svg of document.querySelectorAll('svg')) {
            svg.pauseAnimations();
            svg.setCurrentTime(time);
          }
          for (const { animation, paused } of window.framejetSvg) {
            animation.currentTime = paused ? 0 : time * 1000;
          }
          document.documentElement.getBoundingClientRect();
        }, time);
        if (failures.length)
          throw new Error(`SVG resources failed to load: ${failures.join('; ')}`);
        return page.screenshot({ type: 'png', animations: 'allow', scale: 'css' });
      },
      close,
    };
  } catch (error) {
    await close();
    throw error;
  }
}
