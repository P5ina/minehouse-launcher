
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useSyncExternalStore,
  type CSSProperties,
  type ReactNode,
} from "react";
import * as THREE from "three";
import { cn } from "@/lib/utils";

export interface VoxelArcHandle {
  ripple: (x?: number, y?: number, strength?: number) => void;
  replay: () => void;
}

export interface VoxelArcProps {
  color?: string;
  accentColor?: string;
  backgroundColor?: string;
  density?: number;
  voxelSize?: number;
  roundness?: number;
  tilt?: number;
  horizon?: number;
  curvature?: number;
  waveHeight?: number;
  waveLength?: number;
  speed?: number;
  turbulence?: number;
  brightness?: number;
  glow?: number;
  haze?: number;
  bloom?: number;
  focus?: number;
  aperture?: number;
  grain?: number;
  interactive?: boolean;
  ripples?: number;
  intro?: boolean;
  paused?: boolean;
  dpr?: number;
  children?: ReactNode;
  className?: string;
  style?: CSSProperties;
}

interface Settings {
  color: string;
  accentColor: string;
  backgroundColor: string;
  density: number;
  voxelSize: number;
  roundness: number;
  tilt: number;
  horizon: number;
  curvature: number;
  waveHeight: number;
  waveLength: number;
  speed: number;
  turbulence: number;
  brightness: number;
  glow: number;
  haze: number;
  bloom: number;
  focus: number;
  aperture: number;
  grain: number;
  interactive: boolean;
  ripples: number;
  intro: boolean;
  paused: boolean;
  dpr: number;
  reduced: boolean;
}

interface Controller {
  sync: () => void;
  ripple: (x?: number, y?: number, strength?: number) => void;
  replay: () => void;
  destroy: () => void;
}

const FOV = 26;
const CAMERA_HEIGHT = 3;
const FAR = 18;
const MAX_VOXELS = 110000;
const GRID_W = 192;
const GRID_H = 128;
const STEP = 1 / 120;

const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));
const finite = (value: number, fallback: number) =>
  Number.isFinite(value) ? value : fallback;

const scratchColor = new THREE.Color();

const parseColor = (value: string): [number, number, number] | null => {
  const text = value.trim().toLowerCase();
  if (!text || text === "transparent" || text === "none" || text === "auto") return null;
  try {
    scratchColor.setStyle(text, THREE.NoColorSpace);
  } catch {
    return null;
  }
  return [scratchColor.r, scratchColor.g, scratchColor.b];
};

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
const toGamma = (c: number) => (c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

const toOklab = ([r, g, b]: [number, number, number]) => {
  const lr = toLinear(r);
  const lg = toLinear(g);
  const lb = toLinear(b);
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
};

const fromOklab = ([L, A, B]: number[]): [number, number, number] => {
  const l = Math.pow(L + 0.3963377774 * A + 0.2158037573 * B, 3);
  const m = Math.pow(L - 0.1055613458 * A - 0.0638541728 * B, 3);
  const s = Math.pow(L - 0.0894841775 * A - 1.291485548 * B, 3);
  return [
    clamp(toGamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s), 0, 1),
    clamp(toGamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s), 0, 1),
    clamp(toGamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s), 0, 1),
  ];
};

const subscribeToMotion = (notify: () => void) => {
  const query = window.matchMedia("(prefers-reduced-motion: reduce)");
  query.addEventListener("change", notify);
  return () => query.removeEventListener("change", notify);
};

const readMotion = () =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const passVertex = `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
`;

const voxelVertex = `
attribute vec3 iVoxel;
uniform float uTime;
uniform float uAmp;
uniform float uWaveScale;
uniform float uTurb;
uniform float uCurveX;
uniform float uCurveZ;
uniform float uCell;
uniform float uFill;
uniform float uTilt;
uniform float uFocusDepth;
uniform float uAperture;
uniform float uFocalPx;
uniform vec2 uViewport;
uniform float uNear;
uniform float uFar;
uniform float uSpread;
uniform float uIntro;
uniform float uGlow;
uniform float uBright;
uniform vec3 uColorA;
uniform vec3 uColorB;
uniform float uInk;
uniform sampler2D uRipple;
uniform vec4 uRippleRect;
uniform vec2 uRippleTexel;
uniform float uRippleOn;
varying vec2 vQuad;
varying vec2 vHalf;
varying float vSigma;
varying vec3 vColor;
varying float vAlpha;

const float TAU = 6.28318530718;

vec3 component(vec2 p, vec2 dir, float wavelength, float amp, float rate, float phase) {
  float k = TAU / wavelength;
  vec2 d = normalize(dir);
  float theta = k * dot(d, p) + rate * uTime + phase;
  return vec3(amp * sin(theta), amp * k * cos(theta) * d);
}

vec3 surface(vec2 p) {
  float L = uWaveScale;
  vec3 h = component(p, vec2(0.14, 1.0), 3.3 * L, 0.55, 1.0, 0.0);
  h += component(p, vec2(-0.48, 1.0), 2.15 * L, 0.32, 1.22, 1.9);
  h += component(p, vec2(0.82, 0.62), 1.45 * L, 0.2, 1.5, 4.2);
  h += component(p, vec2(-0.86, 0.42), 0.95 * L, 0.13 * (0.35 + uTurb), 1.95, 2.6);
  h += component(p, vec2(0.33, -0.9), 0.64 * L, 0.09 * uTurb, 2.6, 5.3);
  h += component(p, vec2(-0.2, 0.95), 0.41 * L, 0.05 * uTurb, 3.4, 0.7);
  return h * uAmp;
}

float rippleTap(vec2 st) {
  return texture2D(uRipple, (st + 0.5) * uRippleTexel).r;
}

float rippleSample(vec2 uv) {
  vec2 st = uv / uRippleTexel - 0.5;
  vec2 i = floor(st);
  vec2 f = st - i;
  float a = rippleTap(i);
  float b = rippleTap(i + vec2(1.0, 0.0));
  float c = rippleTap(i + vec2(0.0, 1.0));
  float d = rippleTap(i + vec2(1.0, 1.0));
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

vec3 ripple(vec2 p) {
  if (uRippleOn < 0.5) return vec3(0.0);
  vec2 span = uRippleRect.zw - uRippleRect.xy;
  vec2 uv = (p - uRippleRect.xy) / span;
  if (uv.x <= 0.01 || uv.y <= 0.01 || uv.x >= 0.99 || uv.y >= 0.99) return vec3(0.0);
  float h0 = rippleSample(uv);
  float hx = rippleSample(uv + vec2(uRippleTexel.x, 0.0));
  float hz = rippleSample(uv + vec2(0.0, uRippleTexel.y));
  vec2 cell = span * uRippleTexel;
  return vec3(h0, (hx - h0) / cell.x, (hz - h0) / cell.y);
}

void main() {
  vec2 p = iVoxel.xy;
  vec3 w = surface(p);
  vec3 r = ripple(p);
  float h = w.x + r.x;
  vec2 g = w.yz + r.yz;
  float drop = p.x * p.x * uCurveX + p.y * p.y * uCurveZ;
  vec3 world = vec3(p.x, h - drop, -p.y);
  vec4 viewPos = modelViewMatrix * vec4(world, 1.0);
  float depth = max(-viewPos.z, 0.05);
  vec4 clip = projectionMatrix * viewPos;

  vec3 normal = normalize(vec3(-g.x + 2.0 * p.x * uCurveX, 1.0, g.y - 2.0 * p.y * uCurveZ));
  vec3 toCamera = normalize(cameraPosition - world);
  float facing = clamp(dot(normal, toCamera), 0.06, 1.0);
  float size = uCell * uFill * uFocalPx / depth;
  vec2 core = vec2(size, size * mix(1.0, facing, uTilt));
  float coc = uAperture * uFocalPx * abs(1.0 / depth - 1.0 / uFocusDepth);
  float sigma = max(coc * 0.42, 0.42);
  vec2 extent = core * 0.5 + 2.6 * sigma + 0.5;
  vec2 corner = position.xy * extent;
  gl_Position = clip + vec4(corner / uViewport * 2.0 * clip.w, 0.0, 0.0);
  vQuad = corner;
  vHalf = core * 0.5;
  vSigma = sigma;

  float t = clamp((p.y - uNear) / max(uFar - uNear, 0.001), 0.0, 1.0);
  float side = abs(p.x) / max(p.y * uSpread, 0.001);
  float edge = (1.0 - smoothstep(0.94, 1.0, t)) * (1.0 - smoothstep(0.72, 1.0, side));
  float crest = smoothstep(-0.25, 1.05, h / max(uAmp * 0.62, 0.001));
  float lateral = 1.0 - 0.6 * side * side;
  float limb = exp(-(1.0 - t) * 6.5) * lateral;
  float depthLight = mix(0.45, 1.0, smoothstep(0.0, 0.55, t));
  float front = uIntro * 1.3 - (1.0 - t);
  float reveal = smoothstep(0.0, 0.22, front);
  float sweep = exp(-front * front / 0.004) * step(uIntro, 0.999) * 0.9;
  float energy = (0.018 + 0.9 * pow(crest, 2.4)) * depthLight * lateral + uGlow * limb * (0.25 + 0.95 * crest);
  float light = (energy * reveal + sweep * (0.4 + crest)) * edge * uBright;
  float tone = smoothstep(0.1, 0.95, light);
  vec3 tint = mix(uColorA, uColorB, tone);
  if (uInk > 0.5) {
    vColor = mix(uColorB, uColorA, smoothstep(0.05, 0.7, energy));
    vAlpha = clamp((0.28 + energy * 2.0) * uBright, 0.0, 1.0) * edge * max(reveal, sweep) * (0.55 + 0.45 * depthLight);
  } else {
    vColor = tint * light + vec3(1.0) * max(light - 0.9, 0.0) * 0.45;
    vAlpha = 1.0;
  }
}
`;

const voxelFragment = `
uniform float uRound;
uniform float uInk;
varying vec2 vQuad;
varying vec2 vHalf;
varying float vSigma;
varying vec3 vColor;
varying float vAlpha;

float erfApprox(float x) {
  float x2 = x * x;
  float t = 1.0 - exp(-x2 * (1.27323954 + 0.147 * x2) / (1.0 + 0.147 * x2));
  return sign(x) * sqrt(max(t, 0.0));
}

float blurred(float x, float halfSize, float sigma) {
  float k = 0.70710678 / sigma;
  return 0.5 * (erfApprox((x + halfSize) * k) - erfApprox((x - halfSize) * k));
}

void main() {
  float soft = blurred(vQuad.x, vHalf.x, vSigma) * blurred(vQuad.y, vHalf.y, vSigma);
  float radius = uRound * min(vHalf.x, vHalf.y);
  vec2 q = abs(vQuad) - vHalf + radius;
  float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
  float sharp = 1.0 - smoothstep(-0.6, 0.6, d);
  float k = max(smoothstep(0.55, 1.4, vSigma), 1.0 - smoothstep(1.6, 3.2, min(vHalf.x, vHalf.y) * 2.0));
  float coverage = mix(sharp, soft, k);
  if (coverage < 0.002) discard;
  if (uInk > 0.5) {
    float a = coverage * vAlpha;
    gl_FragColor = vec4(vColor * a, a);
  } else {
    gl_FragColor = vec4(vColor * coverage, 1.0);
  }
}
`;

const downFragment = `
uniform sampler2D uSource;
uniform vec2 uTexel;
uniform float uThreshold;
varying vec2 vUv;
vec3 tap(vec2 d) {
  vec3 c = texture2D(uSource, vUv + d * uTexel).rgb;
  float peak = max(c.r, max(c.g, c.b));
  return c * (max(peak - uThreshold, 0.0) / max(peak, 0.0001));
}
void main() {
  vec3 a = tap(vec2(-2.0, 2.0));
  vec3 b = tap(vec2(0.0, 2.0));
  vec3 c = tap(vec2(2.0, 2.0));
  vec3 d = tap(vec2(-2.0, 0.0));
  vec3 e = tap(vec2(0.0, 0.0));
  vec3 f = tap(vec2(2.0, 0.0));
  vec3 g = tap(vec2(-2.0, -2.0));
  vec3 h = tap(vec2(0.0, -2.0));
  vec3 i = tap(vec2(2.0, -2.0));
  vec3 j = tap(vec2(-1.0, 1.0));
  vec3 k = tap(vec2(1.0, 1.0));
  vec3 l = tap(vec2(-1.0, -1.0));
  vec3 m = tap(vec2(1.0, -1.0));
  vec3 color = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
  gl_FragColor = vec4(color, 1.0);
}
`;

const upFragment = `
uniform sampler2D uSource;
uniform vec2 uTexel;
varying vec2 vUv;
vec3 tap(vec2 d) { return texture2D(uSource, vUv + d * uTexel).rgb; }
void main() {
  vec3 color = tap(vec2(0.0)) * 4.0
    + (tap(vec2(-1.0, 0.0)) + tap(vec2(1.0, 0.0)) + tap(vec2(0.0, -1.0)) + tap(vec2(0.0, 1.0))) * 2.0
    + tap(vec2(-1.0, -1.0)) + tap(vec2(1.0, -1.0)) + tap(vec2(-1.0, 1.0)) + tap(vec2(1.0, 1.0));
  gl_FragColor = vec4(color / 16.0, 1.0);
}
`;

const compositeFragment = `
uniform sampler2D uScene;
uniform sampler2D uBloom;
uniform vec3 uBackground;
uniform vec3 uHazeColor;
uniform vec3 uArc;
uniform float uHaze;
uniform float uBloomStrength;
uniform float uGrain;
uniform float uTime;
uniform float uInk;
uniform float uIntro;
varying vec2 vUv;

vec3 neutral(vec3 color) {
  const float start = 0.76;
  const float desaturation = 0.15;
  float x = min(color.r, min(color.g, color.b));
  float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
  color -= offset;
  float peak = max(color.r, max(color.g, color.b));
  if (peak < start) return color;
  const float d = 1.0 - start;
  float next = 1.0 - d * d / (peak + d - start);
  color *= next / peak;
  float g = 1.0 - 1.0 / (desaturation * (peak - next) + 1.0);
  return mix(color, vec3(next), g);
}

float hash(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

void main() {
  float dx = vUv.x - 0.5;
  float arc = uArc.x - uArc.y * dx * dx;
  float offset = vUv.y - arc;
  float spread = 1.0 - clamp(dx * dx * uArc.z, 0.0, 0.85);
  float profile = offset > 0.0
    ? exp(-offset / 0.045) + exp(-offset / 0.15) * 0.2
    : 1.2 * exp(offset / 0.03);
  float haze = profile * spread * uHaze * uIntro;
  vec4 scene = texture2D(uScene, vUv);
  float n = hash(gl_FragCoord.xy + fract(uTime * 13.0) * 911.0) - 0.5;
  if (uInk > 0.5) {
    float cover = clamp(scene.a, 0.0, 1.0);
    vec3 ink = scene.rgb / max(scene.a, 0.0001);
    vec3 color = mix(uBackground, ink, cover * 0.94);
    color = mix(color, uHazeColor, clamp(haze * 0.16, 0.0, 0.4));
    float mask = smoothstep(0.0, 0.05, cover + haze * 0.2);
    color += n * uGrain * 0.06 * mask;
    gl_FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
    return;
  }
  vec3 emission = scene.rgb + texture2D(uBloom, vUv).rgb * uBloomStrength + uHazeColor * haze;
  vec3 light = neutral(max(emission, 0.0));
  light = pow(light, vec3(1.0 / 2.2));
  vec3 color = 1.0 - (1.0 - uBackground) * (1.0 - light);
  float l = max(light.r, max(light.g, light.b));
  float mask = smoothstep(0.0, 0.04, l);
  color += n * uGrain * 0.1 * mask;
  color += (hash(gl_FragCoord.xy * 1.7) - 0.5) / 255.0 * mask;
  gl_FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
}
`;

function createArc(
  root: HTMLDivElement,
  stage: HTMLDivElement,
  settingsRef: { current: Settings },
): Controller | null {
  const doc = root.ownerDocument;
  const view = doc.defaultView ?? window;
  const canvas = doc.createElement("canvas");
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.cssText = "position:absolute;inset:0;width:100%;height:100%;display:block";
  let renderer: THREE.WebGLRenderer;
  try {
    renderer = new THREE.WebGLRenderer({
      canvas,
      alpha: false,
      antialias: false,
      powerPreference: "high-performance",
    });
  } catch {
    return null;
  }
  stage.appendChild(canvas);
  renderer.autoClear = false;

  const settings = () => settingsRef.current;
  const camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 200);
  camera.position.set(0, CAMERA_HEIGHT, 0);
  const quadCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, -1, 1);
  const quadGeometry = new THREE.PlaneGeometry(2, 2);
  const quadScene = new THREE.Scene();
  const quad = new THREE.Mesh(quadGeometry);
  quad.frustumCulled = false;
  quadScene.add(quad);

  const voxelGeometry = new THREE.InstancedBufferGeometry();
  voxelGeometry.index = quadGeometry.index;
  voxelGeometry.setAttribute("position", quadGeometry.getAttribute("position"));
  let voxelData = new Float32Array(3);
  let voxelAttribute = new THREE.InstancedBufferAttribute(voxelData, 3);
  voxelGeometry.setAttribute("iVoxel", voxelAttribute);
  voxelGeometry.instanceCount = 0;

  let current = new Float32Array(GRID_W * GRID_H);
  let previous = new Float32Array(GRID_W * GRID_H);
  let next = new Float32Array(GRID_W * GRID_H);
  const rippleTexture = new THREE.DataTexture(
    current,
    GRID_W,
    GRID_H,
    THREE.RedFormat,
    THREE.FloatType,
  );
  rippleTexture.minFilter = THREE.NearestFilter;
  rippleTexture.magFilter = THREE.NearestFilter;
  rippleTexture.needsUpdate = true;

  const colorA = new THREE.Vector3(0.2, 0.1, 1);
  const colorB = new THREE.Vector3(0.6, 0.5, 1);
  const background = new THREE.Vector3(0.04, 0.04, 0.04);
  const hazeColor = new THREE.Vector3(0.3, 0.2, 0.9);
  const rippleRect = new THREE.Vector4(-8, 5, 8, FAR);

  const voxelMaterial = new THREE.ShaderMaterial({
    uniforms: {
      uTime: { value: 0 },
      uAmp: { value: 0.18 },
      uWaveScale: { value: 1 },
      uTurb: { value: 0.4 },
      uCurveX: { value: 0.006 },
      uCurveZ: { value: 0.0015 },
      uCell: { value: 0.06 },
      uFill: { value: 0.55 },
      uTilt: { value: 0.6 },
      uFocusDepth: { value: 8 },
      uAperture: { value: 0.1 },
      uFocalPx: { value: 1000 },
      uViewport: { value: new THREE.Vector2(1, 1) },
      uNear: { value: 5 },
      uFar: { value: FAR },
      uSpread: { value: 0.5 },
      uIntro: { value: 1 },
      uGlow: { value: 1 },
      uBright: { value: 1 },
      uColorA: { value: colorA },
      uColorB: { value: colorB },
      uInk: { value: 0 },
      uRound: { value: 0.2 },
      uRipple: { value: rippleTexture },
      uRippleRect: { value: rippleRect },
      uRippleTexel: { value: new THREE.Vector2(1 / GRID_W, 1 / GRID_H) },
      uRippleOn: { value: 0 },
    },
    vertexShader: voxelVertex,
    fragmentShader: voxelFragment,
    transparent: true,
    blending: THREE.AdditiveBlending,
    depthTest: false,
    depthWrite: false,
  });
  const voxelMesh = new THREE.Mesh(voxelGeometry, voxelMaterial);
  voxelMesh.frustumCulled = false;
  const voxelScene = new THREE.Scene();
  voxelScene.add(voxelMesh);

  const target = () =>
    new THREE.WebGLRenderTarget(1, 1, {
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
      stencilBuffer: false,
      minFilter: THREE.LinearFilter,
      magFilter: THREE.LinearFilter,
      generateMipmaps: false,
    });
  const sceneTarget = target();
  const bloomTargets = Array.from({ length: 5 }, target);
  const downMaterial = new THREE.ShaderMaterial({
    uniforms: {
      uSource: { value: null },
      uTexel: { value: new THREE.Vector2() },
      uThreshold: { value: 0 },
    },
    vertexShader: passVertex,
    fragmentShader: downFragment,
    depthTest: false,
    depthWrite: false,
  });
  const upMaterial = new THREE.ShaderMaterial({
    uniforms: { uSource: { value: null }, uTexel: { value: new THREE.Vector2() } },
    vertexShader: passVertex,
    fragmentShader: upFragment,
    blending: THREE.AdditiveBlending,
    transparent: true,
    depthTest: false,
    depthWrite: false,
  });
  const compositeMaterial = new THREE.ShaderMaterial({
    uniforms: {
      uScene: { value: sceneTarget.texture },
      uBloom: { value: bloomTargets[0].texture },
      uBackground: { value: background },
      uHazeColor: { value: hazeColor },
      uArc: { value: new THREE.Vector3(0.44, 0.1, 1) },
      uHaze: { value: 0.8 },
      uBloomStrength: { value: 0.6 },
      uGrain: { value: 0.25 },
      uTime: { value: 0 },
      uInk: { value: 0 },
      uIntro: { value: 1 },
    },
    vertexShader: passVertex,
    fragmentShader: compositeFragment,
    depthTest: false,
    depthWrite: false,
  });

  let width = 0;
  let height = 0;
  let dpr = 1;
  let sized = false;
  let layoutKey = "";
  let colorKey = "";
  let ink = false;
  let page: [number, number, number] = [0.04, 0.04, 0.04];
  let backgroundAt = -10;
  let time = 0;
  let introStart = -1;
  let introDone = false;
  let simActive = false;
  let calmFor = 0;
  let simClock = 0;
  let near = 5;
  let spread = 0.5;
  let raf = 0;
  let last = 0;
  let visible = false;
  let destroyed = false;
  let lost = false;
  let pointerHit: { x: number; z: number; t: number } | null = null;
  const pending: Array<{ x: number; z: number; amount: number; radius: number }> = [];

  const readPage = (): [number, number, number] => {
    let node: HTMLElement | null = root.parentElement;
    while (node) {
      const fill = view.getComputedStyle(node).backgroundColor;
      const match = fill.match(/rgba?\(([^)]+)\)/);
      if (match) {
        const parts = match[1].split(",").map((part) => Number.parseFloat(part));
        if (parts.length < 4 || parts[3] > 0.05)
          return [parts[0] / 255, parts[1] / 255, parts[2] / 255];
      }
      node = node.parentElement;
    }
    const text = view.getComputedStyle(root).color.match(/rgba?\(([^)]+)\)/);
    if (text) {
      const parts = text[1].split(",").map((part) => Number.parseFloat(part));
      return parts[0] + parts[1] + parts[2] > 382 ? [0.04, 0.04, 0.04] : [1, 1, 1];
    }
    return [0.04, 0.04, 0.04];
  };

  const syncColors = () => {
    const s = settings();
    page = parseColor(s.backgroundColor) ?? readPage();
    const key = [page.join(","), s.color, s.accentColor].join("|");
    if (key === colorKey) return;
    colorKey = key;
    ink = 0.2126 * page[0] + 0.7152 * page[1] + 0.0722 * page[2] > 0.55;
    background.set(page[0], page[1], page[2]);
    const first = toOklab(parseColor(s.color) ?? [0.32, 0.15, 1]);
    const second = toOklab(parseColor(s.accentColor) ?? [0.69, 0.62, 0.94]);
    const chromaOf = (lab: number[]) => Math.hypot(lab[1], lab[2]);
    const adapt = (lab: number[], other: number[], lightness: number) => {
      if (!ink) return lab;
      let source = lab;
      if (chromaOf(lab) < 0.03 && chromaOf(other) >= 0.03) source = other;
      const chroma = chromaOf(source);
      if (chroma < 0.03) return [0.3, 0, 0];
      const scale = Math.max(1, 0.14 / chroma);
      return [clamp(source[0], 0.32, lightness), source[1] * scale, source[2] * scale];
    };
    const a = fromOklab(adapt(first, second, 0.5));
    const b = fromOklab(adapt(second, first, 0.6));
    if (ink) {
      colorA.set(a[0], a[1], a[2]);
      colorB.set(b[0], b[1], b[2]);
      hazeColor.set(b[0], b[1], b[2]);
    } else {
      colorA.set(toLinear(a[0]), toLinear(a[1]), toLinear(a[2]));
      colorB.set(toLinear(b[0]), toLinear(b[1]), toLinear(b[2]));
      hazeColor.set(
        (colorA.x * 0.55 + colorB.x * 0.45) * 0.16,
        (colorA.y * 0.55 + colorB.y * 0.45) * 0.16,
        (colorA.z * 0.55 + colorB.z * 0.45) * 0.16,
      );
    }
    voxelMaterial.uniforms.uInk.value = ink ? 1 : 0;
    compositeMaterial.uniforms.uInk.value = ink ? 1 : 0;
  };

  const readSize = () => {
    const s = settings();
    const nextWidth = Math.round(root.clientWidth);
    const nextHeight = Math.round(root.clientHeight);
    if (nextWidth < 2 || nextHeight < 2) {
      sized = false;
      return false;
    }
    const nextDpr = Math.min(view.devicePixelRatio || 1, clamp(finite(s.dpr, 2), 0.5, 3));
    if (sized && nextWidth === width && nextHeight === height && nextDpr === dpr) return false;
    sized = true;
    width = nextWidth;
    height = nextHeight;
    dpr = nextDpr;
    renderer.setPixelRatio(dpr);
    renderer.setSize(width, height, false);
    sceneTarget.setSize(Math.round(width * dpr), Math.round(height * dpr));
    let w = Math.round(width * dpr);
    let h = Math.round(height * dpr);
    for (const bloomTarget of bloomTargets) {
      w = Math.max(1, Math.round(w / 2));
      h = Math.max(1, Math.round(h / 2));
      bloomTarget.setSize(w, h);
    }
    layoutKey = "";
    return true;
  };

  const layout = () => {
    const s = settings();
    const aspect = width / height;
    const key = [
      width,
      height,
      dpr,
      s.horizon,
      s.curvature,
      s.density,
      s.voxelSize,
      s.focus,
      s.aperture,
    ].join("|");
    if (key === layoutKey) return;
    layoutKey = key;
    const vHalf = THREE.MathUtils.degToRad(FOV / 2);
    const tanV = Math.tan(vHalf);
    const tanH = tanV * aspect;
    const curve = clamp(finite(s.curvature, 0.5), 0, 1.5);
    const curveX = curve * 0.034 * Math.pow(1.8 / clamp(aspect, 1.05, 3), 2);
    const curveZ = curve * 0.0012;
    const horizon = clamp(finite(s.horizon, 0.56), 0.12, 0.92);
    const farDrop = FAR * FAR * curveZ;
    const depression = Math.atan((CAMERA_HEIGHT + farDrop) / FAR);
    const above = Math.atan((1 - 2 * horizon) * tanV);
    const pitch = clamp(depression + above, 0.02, 1.2);
    camera.aspect = aspect;
    camera.fov = FOV;
    camera.rotation.set(-pitch, 0, 0);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld(true);
    const bottom = pitch + vHalf;
    near = bottom < Math.PI / 2 - 0.02 ? (CAMERA_HEIGHT / Math.tan(bottom)) * 0.86 : 0.5;
    near = clamp(near, 0.4, FAR - 2);
    spread = tanH * 1.12;
    const focalPx = (height * dpr * 0.5) / tanV;
    const viewDistance = (d: number) => d * Math.cos(pitch) + CAMERA_HEIGHT * Math.sin(pitch);
    const halfWidth = (d: number) => viewDistance(d) * spread;
    const pxPerUnit = (height * 0.5) / tanV / viewDistance(near);
    const gap = clamp(height * 0.0165, 6, 13) / clamp(finite(s.density, 1), 0.3, 3);
    let cell = gap / pxPerUnit;
    const countFor = (spacing: number) => {
      let total = 0;
      for (let d = near; d <= FAR; d += spacing) total += 2 * Math.floor(halfWidth(d) / spacing) + 1;
      return total;
    };
    let total = countFor(cell);
    if (total > MAX_VOXELS) {
      cell *= Math.sqrt(total / MAX_VOXELS) * 1.02;
      total = countFor(cell);
    }
    if (voxelData.length < total * 3) {
      voxelData = new Float32Array(Math.ceil(total * 1.1) * 3);
      voxelAttribute = new THREE.InstancedBufferAttribute(voxelData, 3);
      voxelGeometry.setAttribute("iVoxel", voxelAttribute);
    }
    let index = 0;
    let row = 0;
    for (let d = near; d <= FAR && index < total; d += cell, row++) {
      const columns = Math.floor(halfWidth(d) / cell);
      for (let c = -columns; c <= columns && index < total; c++) {
        voxelData[index * 3] = c * cell;
        voxelData[index * 3 + 1] = d;
        voxelData[index * 3 + 2] = (row * 7919 + c * 104729) % 997;
        index++;
      }
    }
    voxelAttribute.needsUpdate = true;
    voxelGeometry.instanceCount = index;
    const fill = clamp(finite(s.voxelSize, 0.55), 0.1, 1);
    const focus = clamp(finite(s.focus, 0.25), 0, 1);
    const u = voxelMaterial.uniforms;
    u.uCell.value = cell;
    u.uFill.value = fill;
    u.uCurveX.value = curveX;
    u.uCurveZ.value = curveZ;
    u.uFocalPx.value = focalPx;
    u.uViewport.value.set(width * dpr, height * dpr);
    u.uNear.value = near;
    u.uFar.value = FAR;
    u.uSpread.value = spread;
    u.uFocusDepth.value = viewDistance(near + (FAR - near) * focus);
    u.uAperture.value = clamp(finite(s.aperture, 0.35), 0, 2) * 0.16;
    const farHalf = halfWidth(FAR);
    rippleRect.set(-farHalf, near, farHalf, FAR);
    const project = (x: number, d: number) => {
      const point = new THREE.Vector3(x, -(x * x * curveX + d * d * curveZ), -d).project(camera);
      return { x: point.x * 0.5 + 0.5, y: point.y * 0.5 + 0.5 };
    };
    const middle = project(0, FAR);
    const sideX = farHalf * 0.8;
    const sidePoint = project(sideX, FAR);
    const dx = sidePoint.x - 0.5;
    const sag = dx !== 0 ? (middle.y - sidePoint.y) / (dx * dx) : 0;
    compositeMaterial.uniforms.uArc.value.set(middle.y, Math.max(0, sag), 1.4);
  };

  const applyUniforms = () => {
    const s = settings();
    const u = voxelMaterial.uniforms;
    u.uAmp.value = clamp(finite(s.waveHeight, 1), 0, 3) * 0.17;
    u.uWaveScale.value = clamp(finite(s.waveLength, 1), 0.25, 4);
    u.uTurb.value = clamp(finite(s.turbulence, 0.4), 0, 1.5);
    u.uTilt.value = clamp(finite(s.tilt, 1), 0, 1);
    u.uRound.value = clamp(finite(s.roundness, 0.2), 0, 1);
    u.uGlow.value = clamp(finite(s.glow, 1), 0, 3);
    u.uBright.value = clamp(finite(s.brightness, 1), 0, 3);
    compositeMaterial.uniforms.uHaze.value = clamp(finite(s.haze, 0.7), 0, 3);
    compositeMaterial.uniforms.uBloomStrength.value = clamp(finite(s.bloom, 0.5), 0, 3);
    compositeMaterial.uniforms.uGrain.value = clamp(finite(s.grain, 0.25), 0, 1);
  };

  const pick = (clientX: number, clientY: number) => {
    const box = root.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0) return null;
    const nx = ((clientX - box.left) / box.width) * 2 - 1;
    const ny = -(((clientY - box.top) / box.height) * 2 - 1);
    const point = new THREE.Vector3(nx, ny, 0.5).unproject(camera);
    const direction = point.sub(camera.position).normalize();
    if (direction.y >= -0.0001) return null;
    const distance = -camera.position.y / direction.y;
    const x = camera.position.x + direction.x * distance;
    const z = -(camera.position.z + direction.z * distance);
    if (z < near || z > FAR) return null;
    return { x, z };
  };

  const inject = (x: number, z: number, amount: number, radius: number) => {
    const gx = ((x - rippleRect.x) / (rippleRect.z - rippleRect.x)) * GRID_W;
    const gz = ((z - rippleRect.y) / (rippleRect.w - rippleRect.y)) * GRID_H;
    const r = Math.max(1, radius);
    const span = Math.ceil(r * 2.5);
    const cx = Math.round(gx);
    const cz = Math.round(gz);
    for (let j = cz - span; j <= cz + span; j++) {
      if (j < 1 || j >= GRID_H - 1) continue;
      for (let i = cx - span; i <= cx + span; i++) {
        if (i < 1 || i >= GRID_W - 1) continue;
        const d2 = ((i - gx) * (i - gx) + (j - gz) * (j - gz)) / (r * r);
        const value = -amount * Math.exp(-d2);
        current[j * GRID_W + i] += value;
        previous[j * GRID_W + i] += value * 0.6;
      }
    }
    simActive = true;
    calmFor = 0;
    voxelMaterial.uniforms.uRippleOn.value = 1;
  };

  const stepRipples = () => {
    const c2 = 0.1;
    const damp = 0.9962;
    let peak = 0;
    for (let j = 1; j < GRID_H - 1; j++) {
      const row = j * GRID_W;
      for (let i = 1; i < GRID_W - 1; i++) {
        const k = row + i;
        const lap = current[k - 1] + current[k + 1] + current[k - GRID_W] + current[k + GRID_W] - 4 * current[k];
        const value = (2 * current[k] - previous[k] + c2 * lap) * damp;
        next[k] = value;
        const magnitude = value < 0 ? -value : value;
        if (magnitude > peak) peak = magnitude;
      }
    }
    const spare = previous;
    previous = current;
    current = next;
    next = spare;
    return peak;
  };

  const schedule = () => {
    if (destroyed || lost || !visible || raf) return;
    raf = requestAnimationFrame(frame);
  };

  const render = () => {
    if (!sized) return;
    const s = settings();
    if (performance.now() - backgroundAt > 1000) {
      backgroundAt = performance.now();
      syncColors();
    }
    layout();
    applyUniforms();
    voxelMaterial.blending = ink ? THREE.CustomBlending : THREE.AdditiveBlending;
    if (ink) {
      voxelMaterial.blendEquation = THREE.AddEquation;
      voxelMaterial.blendSrc = THREE.OneFactor;
      voxelMaterial.blendDst = THREE.OneFactor;
      voxelMaterial.blendEquationAlpha = THREE.AddEquation;
      voxelMaterial.blendSrcAlpha = THREE.OneFactor;
      voxelMaterial.blendDstAlpha = THREE.OneFactor;
    }
    renderer.setRenderTarget(sceneTarget);
    renderer.setClearColor(0x000000, 0);
    renderer.clear();
    renderer.render(voxelScene, camera);
    const bloom = ink ? 0 : clamp(finite(s.bloom, 0.5), 0, 3);
    if (bloom > 0.001) {
      let source: THREE.WebGLRenderTarget = sceneTarget;
      quad.material = downMaterial;
      for (const bloomTarget of bloomTargets) {
        downMaterial.uniforms.uThreshold.value = source === sceneTarget ? 0.32 : 0;
        downMaterial.uniforms.uSource.value = source.texture;
        downMaterial.uniforms.uTexel.value.set(1 / source.width, 1 / source.height);
        renderer.setRenderTarget(bloomTarget);
        renderer.render(quadScene, quadCamera);
        source = bloomTarget;
      }
      quad.material = upMaterial;
      for (let i = bloomTargets.length - 1; i > 0; i--) {
        upMaterial.uniforms.uSource.value = bloomTargets[i].texture;
        upMaterial.uniforms.uTexel.value.set(1 / bloomTargets[i].width, 1 / bloomTargets[i].height);
        renderer.setRenderTarget(bloomTargets[i - 1]);
        renderer.render(quadScene, quadCamera);
      }
    }
    compositeMaterial.uniforms.uBloomStrength.value = bloom;
    compositeMaterial.uniforms.uTime.value = time;
    quad.material = compositeMaterial;
    renderer.setRenderTarget(null);
    renderer.render(quadScene, quadCamera);
  };

  const frame = (now: number) => {
    raf = 0;
    if (destroyed || lost) return;
    const s = settings();
    const dt =
      last && now - last < 100 ? Math.min(Math.max((now - last) / 1000, 0), 0.05) : 1 / 60;
    last = now;
    const moving = !s.reduced && !s.paused;
    if (moving) time += dt * clamp(finite(s.speed, 0.5), 0, 4);
    voxelMaterial.uniforms.uTime.value = time;
    if (s.intro && !s.reduced && !introDone) {
      if (introStart < 0) introStart = now;
      const progress = clamp((now - introStart) / 2600, 0, 1);
      voxelMaterial.uniforms.uIntro.value = progress;
      compositeMaterial.uniforms.uIntro.value = clamp(progress * 1.25, 0, 1);
      if (progress >= 1) introDone = true;
    } else {
      introDone = true;
      voxelMaterial.uniforms.uIntro.value = 1;
      compositeMaterial.uniforms.uIntro.value = 1;
    }
    for (const drop of pending.splice(0)) inject(drop.x, drop.z, drop.amount, drop.radius);
    if (simActive && moving) {
      simClock += dt;
      let peak = 1;
      let steps = 0;
      while (simClock >= STEP && steps < 4) {
        peak = stepRipples();
        simClock -= STEP;
        steps++;
      }
      if (steps >= 4) simClock = 0;
      if (steps > 0) {
        rippleTexture.image.data = current;
        rippleTexture.needsUpdate = true;
      }
      if (peak < 0.0004) {
        calmFor += dt;
        if (calmFor > 0.6) {
          simActive = false;
          current.fill(0);
          previous.fill(0);
          next.fill(0);
          rippleTexture.image.data = current;
          rippleTexture.needsUpdate = true;
          voxelMaterial.uniforms.uRippleOn.value = 0;
        }
      } else calmFor = 0;
    }
    render();
    const animating =
      (moving && clamp(finite(s.speed, 0.5), 0, 4) > 0) ||
      (simActive && moving) ||
      !introDone;
    if (animating && !document.hidden) raf = requestAnimationFrame(frame);
    else last = 0;
  };

  const isControl = (target: EventTarget | null) => {
    const element = target as Element | null;
    return (
      !!element &&
      typeof element.closest === "function" &&
      !!element.closest("a,button,input,textarea,select,label,summary,[role=button],[contenteditable=true]")
    );
  };

  const onPointerMove = (event: PointerEvent) => {
    const s = settings();
    if (!s.interactive || s.reduced || s.paused) return;
    const hit = pick(event.clientX, event.clientY);
    const now = performance.now();
    if (!hit) {
      pointerHit = null;
      return;
    }
    const strength = clamp(finite(s.ripples, 1), 0, 3);
    if (pointerHit && strength > 0) {
      const dx = hit.x - pointerHit.x;
      const dz = hit.z - pointerHit.z;
      const distance = Math.hypot(dx, dz);
      const elapsed = Math.max(0.008, (now - pointerHit.t) / 1000);
      const speed = distance / elapsed;
      const amount = clamp(speed * 0.009, 0, 0.07) * strength;
      if (amount > 0.0005) {
        const cellSize = (rippleRect.z - rippleRect.x) / GRID_W;
        const steps = Math.min(12, Math.max(1, Math.ceil(distance / (cellSize * 2))));
        for (let i = 1; i <= steps; i++) {
          pending.push({
            x: pointerHit.x + (dx * i) / steps,
            z: pointerHit.z + (dz * i) / steps,
            amount: amount / Math.sqrt(steps),
            radius: 2.2,
          });
        }
        schedule();
      }
    }
    pointerHit = { x: hit.x, z: hit.z, t: now };
  };

  const onPointerLeave = () => {
    pointerHit = null;
  };

  const onPointerDown = (event: PointerEvent) => {
    const s = settings();
    if (!s.interactive || s.reduced || s.paused || !event.isPrimary) return;
    if (event.pointerType === "mouse" && event.button !== 0) return;
    if (isControl(event.target)) return;
    const hit = pick(event.clientX, event.clientY);
    if (!hit) return;
    const strength = clamp(finite(s.ripples, 1), 0, 3);
    if (strength <= 0) return;
    pending.push({ x: hit.x, z: hit.z, amount: 0.42 * strength, radius: 3.6 });
    schedule();
  };

  const onVisibility = () => {
    last = 0;
    schedule();
  };

  const onLost = (event: Event) => {
    event.preventDefault();
    lost = true;
    cancelAnimationFrame(raf);
    raf = 0;
  };

  const onRestored = () => {
    lost = false;
    layoutKey = "";
    rippleTexture.needsUpdate = true;
    schedule();
  };

  const resizeObserver = new ResizeObserver(() => {
    if (readSize()) {
      render();
      schedule();
    }
  });

  const intersection = new IntersectionObserver(
    ([entry]) => {
      visible = entry.isIntersecting;
      last = 0;
      schedule();
    },
    { rootMargin: "120px" },
  );

  root.addEventListener("pointermove", onPointerMove);
  root.addEventListener("pointerdown", onPointerDown);
  root.addEventListener("pointerleave", onPointerLeave);
  root.addEventListener("pointercancel", onPointerLeave);
  document.addEventListener("visibilitychange", onVisibility);
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestored);
  resizeObserver.observe(root);
  intersection.observe(root);
  readSize();
  syncColors();

  return {
    sync: () => {
      backgroundAt = -10;
      if (readSize()) layoutKey = "";
      last = 0;
      if (!raf && visible) {
        render();
        schedule();
      }
    },
    ripple: (x, y, strength) => {
      const s = settings();
      if (s.reduced) return;
      const box = root.getBoundingClientRect();
      const hit = pick(
        box.left + clamp(x ?? 0.5, 0, 1) * box.width,
        box.top + clamp(y ?? 0.8, 0, 1) * box.height,
      );
      if (!hit) return;
      pending.push({
        x: hit.x,
        z: hit.z,
        amount: 0.42 * clamp(finite(strength ?? 1, 1), 0, 3),
        radius: 3.6,
      });
      schedule();
    },
    replay: () => {
      introStart = -1;
      introDone = false;
      last = 0;
      schedule();
    },
    destroy: () => {
      destroyed = true;
      cancelAnimationFrame(raf);
      resizeObserver.disconnect();
      intersection.disconnect();
      root.removeEventListener("pointermove", onPointerMove);
      root.removeEventListener("pointerdown", onPointerDown);
      root.removeEventListener("pointerleave", onPointerLeave);
      root.removeEventListener("pointercancel", onPointerLeave);
      document.removeEventListener("visibilitychange", onVisibility);
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestored);
      voxelGeometry.dispose();
      quadGeometry.dispose();
      voxelMaterial.dispose();
      downMaterial.dispose();
      upMaterial.dispose();
      compositeMaterial.dispose();
      rippleTexture.dispose();
      sceneTarget.dispose();
      for (const bloomTarget of bloomTargets) bloomTarget.dispose();
      renderer.dispose();
      canvas.remove();
    },
  };
}

export const VoxelArc = forwardRef<VoxelArcHandle, VoxelArcProps>(function VoxelArc(
  {
    color = "#5227FF",
    accentColor = "#B19EEF",
    backgroundColor = "transparent",
    density = 1,
    voxelSize = 0.55,
    roundness = 0.2,
    tilt = 1,
    horizon = 0.56,
    curvature = 0.5,
    waveHeight = 1,
    waveLength = 1,
    speed = 0.5,
    turbulence = 0.4,
    brightness = 1,
    glow = 1,
    haze = 0.7,
    bloom = 0.5,
    focus = 0.25,
    aperture = 0.35,
    grain = 0.25,
    interactive = true,
    ripples = 1,
    intro = true,
    paused = false,
    dpr = 2,
    children,
    className,
    style,
  },
  ref,
) {
  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const controllerRef = useRef<Controller | null>(null);
  const reduced = useSyncExternalStore(subscribeToMotion, readMotion, () => false);

  const settings: Settings = {
    color,
    accentColor,
    backgroundColor,
    density,
    voxelSize,
    roundness,
    tilt,
    horizon,
    curvature,
    waveHeight,
    waveLength,
    speed,
    turbulence,
    brightness,
    glow,
    haze,
    bloom,
    focus,
    aperture,
    grain,
    interactive,
    ripples,
    intro,
    paused,
    dpr,
    reduced,
  };
  const settingsRef = useRef(settings);

  useEffect(() => {
    settingsRef.current = settings;
    controllerRef.current?.sync();
  });

  useEffect(() => {
    const root = rootRef.current;
    const stage = stageRef.current;
    if (!root || !stage) return;
    const controller = createArc(root, stage, settingsRef);
    controllerRef.current = controller;
    return () => {
      controller?.destroy();
      controllerRef.current = null;
    };
  }, []);

  useImperativeHandle(
    ref,
    () => ({
      ripple: (x?: number, y?: number, strength?: number) =>
        controllerRef.current?.ripple(x, y, strength),
      replay: () => controllerRef.current?.replay(),
    }),
    [],
  );

  const backgroundValue = backgroundColor.trim().toLowerCase();
  const transparent = backgroundValue === "transparent" || backgroundValue === "";

  return (
    <div
      ref={rootRef}
      className={cn("relative isolate h-full min-h-[320px] w-full overflow-hidden", className)}
      style={{
        backgroundColor: transparent ? undefined : backgroundColor,
        touchAction: "pan-y",
        ...style,
      }}
    >
      <div
        ref={stageRef}
        aria-hidden="true"
        style={{ position: "absolute", inset: 0, pointerEvents: "none" }}
      />
      {children ? (
        <div style={{ position: "relative", zIndex: 1, height: "100%" }}>{children}</div>
      ) : null}
    </div>
  );
});

VoxelArc.displayName = "VoxelArc";

export default VoxelArc;
