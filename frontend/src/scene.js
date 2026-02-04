/**
 * Three.js scene for rendering 3D pose skeletons.
 * Uses OrbitControls for pan / zoom / rotate. Includes a floor grid and XYZ axes.
 * Convention: world up = +Z (consistent with backend and pose data).
 */

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { getVisibleEdges } from './poseSkeleton.js';

const BONE_COLOR = 0x00ccff;
const JOINT_COLOR = 0xff6600;
const JOINT_RADIUS = 0.02;
const JOINT_PICK_RADIUS = 0.12;
const BONE_RADIUS = 0.008;

const GRID_SIZE = 2;
const GRID_DIVISIONS = 20;
const AXES_SIZE = 0.8;

const FRUSTUM_COLOR = 0x9e6a03;
const FRUSTUM_HIGHLIGHT_COLOR = 0x58a6ff;
const PICK_SPHERE_RADIUS = 0.025;
const JOINT_HIGHLIGHT_COLOR = 0x58a6ff;

/**
 * @param {HTMLDivElement} container
 * @param {{ onCameraClick?: (cameraName: string | null) => void, onKeypointClick?: (keypointIndex: number | null) => void }} [options]
 * @returns {{ update: (msg: import('./poseMessage.js').PoseStreamMessage) => void, setHighlight: (cameraName: string | null) => void, setKeypointHighlight: (keypointIndex: number | null) => void, dispose: () => void, render: () => void }}
 */
export function createPoseScene(container, options = {}) {
  const { onCameraClick, onKeypointClick } = options;
  const raycaster = new THREE.Raycaster();
  const mouse = new THREE.Vector2();
  const width = container.clientWidth;
  const height = container.clientHeight;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0d1117);

  const camera = new THREE.PerspectiveCamera(50, width / height, 0.01, 100);
  camera.up.set(0, 0, 1); // Z up
  // ~3m from target (0,0,1): direction (1,1,1).normalize() * 3
  const dist = 3;
  const dir = new THREE.Vector3(1, 0.5, 0.5).normalize().multiplyScalar(dist);
  camera.position.set(0, 0, 1).add(dir);
  camera.lookAt(0, 0, 1);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(width, height);
  renderer.setPixelRatio(window.devicePixelRatio);
  container.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.05;
  controls.target.set(0, 0, 1);
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

  const cameraGroup = new THREE.Group();
  scene.add(cameraGroup);

  /** @type {THREE.Mesh[]} */
  const jointMeshes = [];
  /** @type {THREE.Mesh[]} - larger spheres for raycast picking */
  const jointPickSpheres = [];
  /** @type {THREE.Mesh[]} */
  const boneMeshes = [];
  /** @type {THREE.Object3D[]} */
  const cameraObjects = [];
  /** @type {{ name: string, line: THREE.LineSegments, sphere: THREE.Mesh }[]} */
  const cameraEntries = [];

  // Pose data is already Z-up (e.g. fake_poses/running_pose.csv: z ≈ 0.5–4.2 vertical).
  // Viewer is Z-up and grid in XY, so use (x, y, z) directly.
  function addJoint(x, y, z, keypointIndex) {
    const geo = new THREE.SphereGeometry(JOINT_RADIUS, 12, 12);
    const mat = new THREE.MeshBasicMaterial({ color: JOINT_COLOR });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.position.set(x, y, z);
    mesh.userData.keypointIndex = keypointIndex;
    group.add(mesh);
    jointMeshes.push(mesh);
    const pickGeo = new THREE.SphereGeometry(JOINT_PICK_RADIUS, 8, 8);
    const pickMat = new THREE.MeshBasicMaterial({ transparent: true, opacity: 0, depthWrite: false });
    const pickMesh = new THREE.Mesh(pickGeo, pickMat);
    pickMesh.position.set(x, y, z);
    pickMesh.userData.keypointIndex = keypointIndex;
    group.add(pickMesh);
    jointPickSpheres.push(pickMesh);
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

  function clearPoses() {
    jointMeshes.forEach((m) => {
      m.geometry.dispose();
      if (m.material.dispose) m.material.dispose();
      group.remove(m);
    });
    jointMeshes.length = 0;
    jointPickSpheres.forEach((m) => {
      m.geometry.dispose();
      if (m.material.dispose) m.material.dispose();
      group.remove(m);
    });
    jointPickSpheres.length = 0;
    boneMeshes.forEach((m) => {
      m.geometry.dispose();
      if (m.material.dispose) m.material.dispose();
      group.remove(m);
    });
    boneMeshes.length = 0;
  }

  function clearCameras() {
    cameraObjects.forEach((o) => {
      if (o.geometry?.dispose) o.geometry.dispose();
      if (o.material?.dispose) o.material.dispose();
      cameraGroup.remove(o);
    });
    cameraObjects.length = 0;
    cameraEntries.length = 0;
  }

  function setHighlight(cameraName) {
    cameraEntries.forEach(({ name, line, sphere }) => {
      const highlight = name === cameraName;
      line.material.color.setHex(highlight ? FRUSTUM_HIGHLIGHT_COLOR : FRUSTUM_COLOR);
      sphere.material.color.setHex(highlight ? FRUSTUM_HIGHLIGHT_COLOR : FRUSTUM_COLOR);
    });
  }

  function onPointerMove(event) {
    const rect = renderer.domElement.getBoundingClientRect();
    mouse.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    mouse.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
  }

  function onPointerDown(event) {
    if (event.button !== 0) return;
    const rect = renderer.domElement.getBoundingClientRect();
    mouse.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    mouse.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(mouse, camera);
    const cameraPickables = cameraGroup.children.filter((o) => o.userData.cameraName != null);
    const cameraHits = raycaster.intersectObjects(cameraPickables);
    if (cameraHits.length > 0) {
      const name = cameraHits[0].object.userData.cameraName;
      event.stopPropagation();
      event.preventDefault();
      if (name != null && onCameraClick) onCameraClick(name);
      return;
    }
    let jointHits = raycaster.intersectObjects(jointPickSpheres);
    if (jointHits.length === 0 && jointMeshes.length > 0) {
      jointHits = raycaster.intersectObjects(jointMeshes);
    }
    if (jointHits.length > 0) {
      const idx = jointHits[0].object.userData.keypointIndex;
      event.stopPropagation();
      event.preventDefault();
      if (typeof idx === 'number' && onKeypointClick) onKeypointClick(idx);
      return;
    }
    if (onCameraClick) onCameraClick(null);
    if (onKeypointClick) onKeypointClick(null);
  }

  renderer.domElement.addEventListener('pointermove', onPointerMove);
  renderer.domElement.addEventListener('pointerdown', onPointerDown, true);

  function addCameraModel(cam) {
    const cameraName = typeof cam.camera_name === 'string' ? cam.camera_name : '';
    const pos = new THREE.Vector3(cam.position.x, cam.position.y, cam.position.z);
    const right = new THREE.Vector3(cam.right.x, cam.right.y, cam.right.z).normalize();
    const up = new THREE.Vector3(cam.up.x, cam.up.y, cam.up.z).normalize();
    const forward = new THREE.Vector3(cam.forward.x, cam.forward.y, cam.forward.z).normalize();

    const d = 0.35;
    const fx = typeof cam.fx === 'number' && cam.fx > 0 ? cam.fx : 600;
    const fy = typeof cam.fy === 'number' && cam.fy > 0 ? cam.fy : 600;
    const cx = typeof cam.cx === 'number' ? cam.cx : 320;
    const cy = typeof cam.cy === 'number' ? cam.cy : 240;
    const width = typeof cam.width_px === 'number' && cam.width_px > 0 ? cam.width_px : Math.max(1, 2 * cx);
    const height = typeof cam.height_px === 'number' && cam.height_px > 0 ? cam.height_px : Math.max(1, 2 * cy);

    function cornerWorld(u, v) {
      const x = ((u - cx) / fx) * d;
      const y = -((v - cy) / fy) * d; // v down → y up
      const z = d;
      return pos
        .clone()
        .add(right.clone().multiplyScalar(x))
        .add(up.clone().multiplyScalar(y))
        .add(forward.clone().multiplyScalar(z));
    }

    const c1 = cornerWorld(0, 0);
    const c2 = cornerWorld(width, 0);
    const c3 = cornerWorld(width, height);
    const c4 = cornerWorld(0, height);

    const points = [
      pos, c1, pos, c2, pos, c3, pos, c4, // rays
      c1, c2, c2, c3, c3, c4, c4, c1,     // rectangle
    ];
    const geo = new THREE.BufferGeometry().setFromPoints(points);
    const mat = new THREE.LineBasicMaterial({ color: FRUSTUM_COLOR, linewidth: 2 });
    const line = new THREE.LineSegments(geo, mat);
    line.userData.cameraName = cameraName;
    cameraGroup.add(line);
    cameraObjects.push(line);

    const axes = new THREE.AxesHelper(0.12);
    axes.position.copy(pos);
    axes.userData.cameraName = cameraName;
    cameraGroup.add(axes);
    cameraObjects.push(axes);

    const sphereGeo = new THREE.SphereGeometry(PICK_SPHERE_RADIUS, 12, 12);
    const sphereMat = new THREE.MeshBasicMaterial({ color: FRUSTUM_COLOR });
    const sphere = new THREE.Mesh(sphereGeo, sphereMat);
    sphere.position.copy(pos);
    sphere.userData.cameraName = cameraName;
    cameraGroup.add(sphere);
    cameraObjects.push(sphere);

    cameraEntries.push({ name: cameraName, line, sphere });
  }

  function update(msg) {
    // Only replace cameras when we receive a non-empty list (sent on connect / camera register).
    // Pose updates send cameras: [], so we must not clear cameras on every message.
    if (Array.isArray(msg.cameras) && msg.cameras.length > 0) {
      clearCameras();
      for (const c of msg.cameras) addCameraModel(c);
    }
    clearPoses();
    if (!msg.poses || msg.poses.length === 0) return;
    const pose = msg.poses[0];
    const keypoints = pose.keypoints;
    const edges = getVisibleEdges(keypoints);
    for (const [a, b] of edges) {
      const pa = keypoints[a];
      const pb = keypoints[b];
      if (pa && pb) addBone(pa.x, pa.y, pa.z, pb.x, pb.y, pb.z);
    }
    for (let i = 0; i < keypoints.length; i++) {
      const p = keypoints[i];
      if (p) addJoint(p.x, p.y, p.z, i);
    }
  }

  function setKeypointHighlight(keypointIndex) {
    jointMeshes.forEach((mesh) => {
      const highlight = mesh.userData.keypointIndex === keypointIndex;
      mesh.material.color.setHex(highlight ? JOINT_HIGHLIGHT_COLOR : JOINT_COLOR);
    });
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
    setHighlight,
    setKeypointHighlight,
    dispose: () => {
      window.removeEventListener('resize', onResize);
      renderer.domElement.removeEventListener('pointermove', onPointerMove);
      renderer.domElement.removeEventListener('pointerdown', onPointerDown);
      controls.dispose();
      clearPoses();
      clearCameras();
      renderer.dispose();
      if (container.contains(renderer.domElement)) container.removeChild(renderer.domElement);
    },
    render: () => {
      controls.update();
      renderer.render(scene, camera);
    },
  };
}
