export const video = { width: 1920, height: 1080, fps: 30, duration: 4 };

// A busier scene than the orbit example: many draw calls, per-object JavaScript and shaded lighting.
export function setup({ THREE, width, height }) {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color('#0b1220');
  const camera = new THREE.PerspectiveCamera(42, width / height, 0.1, 200);
  camera.position.set(0, 9, 26);
  camera.lookAt(0, 0, 0);
  const geometry = new THREE.IcosahedronGeometry(0.42, 3);
  const materials = ['#53eac5', '#6c9cff', '#f1f6ff', '#ff8a5c'].map(color =>
    new THREE.MeshStandardMaterial({ color, roughness: 0.3, metalness: 0.55 }));
  const meshes = Array.from({ length: 1400 }, (_, index) => {
    const mesh = new THREE.Mesh(geometry, materials[index % materials.length]);
    scene.add(mesh);
    return mesh;
  });
  scene.add(new THREE.HemisphereLight('#c7e7ff', '#172343', 1.6));
  for (const [x, y, z, color] of [[12, 14, 10, '#ffffff'], [-14, 6, -8, '#6f8dff'], [0, -10, 14, '#ff9d7a']]) {
    const light = new THREE.PointLight(color, 300);
    light.position.set(x, y, z);
    scene.add(light);
  }
  return {
    scene,
    camera,
    update({ time }) {
      meshes.forEach((mesh, index) => {
        const ring = 2 + index % 13;
        const angle = time * (0.4 + (index % 7) * 0.08) + index * 2.399963;
        mesh.position.set(Math.cos(angle) * ring, Math.sin(time + index) * 3, Math.sin(angle) * ring);
        mesh.rotation.set(angle, time, index);
      });
    },
    dispose() {
      geometry.dispose();
      for (const material of materials) material.dispose();
    },
  };
}
