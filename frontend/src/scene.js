/**
 * Three.js scene for rendering 3D pose skeletons.
 * Uses OrbitControls for pan / zoom / rotate. Includes a floor grid and XYZ axes.
 */

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { getVisibleEdges } from './poseSkeleton.js';

const BONE_COLOR = 0x00ccff;
const JOINT_COLOR = 0xff6600;
const JOINT_RADIUS = 0.02;
const BONE_RADIUS = 0.008;

const GRID_SIZE = 2;
const GRID_DIVISIONS = 20;
const AXES_SIZE = 0.8;

/**
 * @param {HTMLDivElement} container
 * @returns {{ update: (msg: import('./poseMessage.js').PoseStreamMessage) => void, dispose: () => void }}
 */
export function createPoseScene(container) {
  const width = container.clientWidth;
  const height = container.clientHeight;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0d1117);

  const camera = new THREE.PerspectiveCamera(50, width / height, 0.01, 100);
  camera.up.set(0, 0, 1); // Z up
  camera.position.set(1.5, 1, 1.5);
  camera.lookAt(0, 0, 0.5);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(width, height);
  renderer.setPixelRatio(window.devicePixelRatio);
  container.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.05;
  controls.target.set(0, 0, 0.5);
  controls.minDistance = 0.2;
  controls.maxDistance = 20;

  const gridHelper = new THREE.GridHelper(GRID_SIZE, GRID_DIVISIONS, 0x30363d, 0x21262d);
  gridHelper.rotation.x = -Math.PI / 2; // XY plane (Z up → floor at z=0)
  gridHelper.position.z = 0;
  scene.add(gridHelper);

  const axesHelper = new THREE.AxesHelper(AXES_SIZE);
  scene.add(axesHelper);

  const group = new THREE.Group();
  scene.add(group);

  /** @type {THREE.Mesh[]} */
  const jointMeshes = [];
  /** @type {THREE.Mesh[]} */
  const boneMeshes = [];

  // Pose data is already Z-up (e.g. fake_poses/running_pose.csv: z ≈ 0.5–4.2 vertical).
  // Viewer is Z-up and grid in XY, so use (x, y, z) directly.
  function addJoint(x, y, z) {
    const geo = new THREE.SphereGeometry(JOINT_RADIUS, 12, 12);
    const mat = new THREE.MeshBasicMaterial({ color: JOINT_COLOR });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.position.set(x, y, z);
    group.add(mesh);
    jointMeshes.push(mesh);
  }

  function addBone(ax, ay, az, bx, by, bz) {
    const geo = new THREE.BufferGeometry().setFromPoints([
      new THREE.Vector3(ax, ay, az),
      new THREE.Vector3(bx, by, bz),
    ]);
    const mat = new THREE.LineBasicMaterial({ color: BONE_COLOR, linewidth: 2 });
    const line = new THREE.Line(geo, mat);
    group.add(line);
    boneMeshes.push(line);
  }

  function clear() {
    jointMeshes.forEach((m) => {
      m.geometry.dispose();
      if (m.material.dispose) m.material.dispose();
      group.remove(m);
    });
    jointMeshes.length = 0;
    boneMeshes.forEach((m) => {
      m.geometry.dispose();
      if (m.material.dispose) m.material.dispose();
      group.remove(m);
    });
    boneMeshes.length = 0;
  }

  function update(msg) {
    clear();
    if (!msg.poses || msg.poses.length === 0) return;
    const pose = msg.poses[0];
    const keypoints = pose.keypoints;
    const edges = getVisibleEdges(keypoints);
    for (const [a, b] of edges) {
      const pa = keypoints[a];
      const pb = keypoints[b];
      if (pa && pb) addBone(pa.x, pa.y, pa.z, pb.x, pb.y, pb.z);
    }
    for (const p of keypoints) {
      if (p) addJoint(p.x, p.y, p.z);
    }
  }

  function onResize() {
    const w = container.clientWidth;
    const h = container.clientHeight;
    camera.aspect = w / h;
    camera.updateProjection();
    renderer.setSize(w, h);
  }

  window.addEventListener('resize', onResize);

  return {
    update,
    dispose: () => {
      window.removeEventListener('resize', onResize);
      controls.dispose();
      clear();
      renderer.dispose();
      if (container.contains(renderer.domElement)) container.removeChild(renderer.domElement);
    },
    render: () => {
      controls.update();
      renderer.render(scene, camera);
    },
  };
}
