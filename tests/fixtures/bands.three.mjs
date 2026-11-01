export { video } from './metadata.mjs';

export async function setup({ THREE }) {
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10);
  camera.position.z = 2;
  const texture = await new THREE.TextureLoader().loadAsync('./bands.svg');
  texture.colorSpace = THREE.SRGBColorSpace;
  scene.add(new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.MeshBasicMaterial({ map: texture })));
  const marker = new THREE.Mesh(new THREE.PlaneGeometry(0.2, 0.2), new THREE.MeshBasicMaterial({ color: '#ffffff' }));
  marker.position.z = 0.1;
  scene.add(marker);
  return { scene, camera, update({ time }) { marker.position.x = -0.7 + time * 1.4; } };
}
