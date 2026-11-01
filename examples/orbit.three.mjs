export const video = { width: 960, height: 540, fps: 30, duration: 4 };

export function setup({ THREE, width, height, renderer }) {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color('#101827');
  const camera = new THREE.PerspectiveCamera(38, width / height, 0.1, 100);
  camera.position.set(0, 1.4, 6.5);
  camera.lookAt(0, 0, 0);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;

  const sculpture = new THREE.Group();
  const geometry = new THREE.TorusKnotGeometry(0.85, 0.24, 192, 32);
  const material = new THREE.MeshStandardMaterial({ color: '#53eac5', roughness: 0.26, metalness: 0.45 });
  sculpture.add(new THREE.Mesh(geometry, material));
  scene.add(sculpture);

  const ringGeometry = new THREE.TorusGeometry(1.65, 0.016, 12, 160);
  const ringMaterial = new THREE.MeshBasicMaterial({ color: '#5273ac' });
  const ring = new THREE.Mesh(ringGeometry, ringMaterial);
  ring.rotation.x = Math.PI / 2.6;
  scene.add(ring);

  const satelliteGeometry = new THREE.IcosahedronGeometry(0.13, 1);
  const satelliteMaterial = new THREE.MeshStandardMaterial({ color: '#d4e5ff', roughness: 0.3 });
  const satellite = new THREE.Mesh(satelliteGeometry, satelliteMaterial);
  scene.add(satellite);

  scene.add(new THREE.HemisphereLight('#c7e7ff', '#172343', 2.3));
  const light = new THREE.DirectionalLight('#ffffff', 5);
  light.position.set(3, 4, 5);
  scene.add(light);
  const rim = new THREE.DirectionalLight('#6f8dff', 4);
  rim.position.set(-4, 1, -3);
  scene.add(rim);

  return {
    scene,
    camera,
    update({ time, duration }) {
      const angle = time / duration * Math.PI * 2;
      sculpture.rotation.set(Math.sin(angle) * 0.25, angle, 0);
      satellite.position.set(Math.cos(angle) * 1.65, Math.sin(angle) * 0.6, Math.sin(angle) * 1.5);
    },
    dispose() {
      for (const resource of [geometry, material, ringGeometry, ringMaterial, satelliteGeometry, satelliteMaterial]) resource.dispose();
    },
  };
}
