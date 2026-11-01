import { readFile } from 'node:fs/promises';

export const video = { width:96, height:96, fps:4, duration:2 };
const animation = await readFile(new URL('./animated.svg', import.meta.url), 'utf8');
export function render() { return animation; }
