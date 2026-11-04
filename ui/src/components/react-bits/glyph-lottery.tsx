
import {
  forwardRef,
  useEffect,
  useId,
  useImperativeHandle,
  useMemo,
  useRef,
  useSyncExternalStore,
  type CSSProperties,
  type RefObject,
} from "react";
import { cn } from "@/lib/utils";

export type GlyphLotteryTag = "span" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6";
export type GlyphLotteryDirection = "left" | "right" | "center" | "edges" | "random";
export type GlyphLotteryTrigger = "view" | "mount" | "manual";

export interface GlyphLotteryHandle {
  spin: () => void;
}

export interface GlyphLotteryProps {
  text?: string;
  as?: GlyphLotteryTag;
  fontSize?: number | string;
  fontWeight?: number;
  letterSpacing?: string;
  lineHeight?: number;
  align?: "left" | "center" | "right";
  color?: string;
  accentColor?: string;
  charset?: string;
  trigger?: GlyphLotteryTrigger;
  direction?: GlyphLotteryDirection;
  spinSpeed?: number;
  duration?: number;
  stagger?: number;
  bounce?: number;
  blur?: number;
  fade?: number;
  flash?: number;
  loop?: boolean;
  interval?: number;
  spinOnHover?: boolean;
  clickToSpin?: boolean;
  flickable?: boolean;
  onComplete?: () => void;
  className?: string;
  style?: CSSProperties;
}

interface Settings {
  accentColor: string;
  charset: string;
  trigger: GlyphLotteryTrigger;
  direction: GlyphLotteryDirection;
  spinSpeed: number;
  duration: number;
  stagger: number;
  bounce: number;
  blur: number;
  fade: number;
  flash: number;
  loop: boolean;
  interval: number;
  spinOnHover: boolean;
  clickToSpin: boolean;
  flickable: boolean;
  reduced: boolean;
  onComplete?: () => void;
}

interface Reel {
  root: HTMLElement;
  strip: HTMLElement;
  cells: HTMLElement[];
  target: string;
  pool: string[];
  seed: number;
  pos: number;
  vel: number;
  goal: number;
  stopAt: number;
  mode: "rest" | "spin" | "coast" | "settle" | "drag";
  shown: number;
  glow: number;
  order: number;
  dragY: number;
  dragPos: number;
  history: { t: number; pos: number }[];
}

interface Controller {
  measure: () => void;
  sync: () => void;
  destroy: () => void;
  spin: () => void;
}

const BLUR_LEVELS = [0.6, 1.2, 2.2, 3.6];
const UPPER = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWER = "abcdefghijklmnopqrstuvwxyz";
const DIGITS = "0123456789";
const SYMBOLS = "!?#$%&*+=/<>@";

const subscribeToMotion = (notify: () => void) => {
  const media = window.matchMedia("(prefers-reduced-motion: reduce)");
  media.addEventListener("change", notify);
  return () => media.removeEventListener("change", notify);
};

const readMotion = () =>
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));

const segment = (value: string) =>
  typeof Intl === "undefined" || typeof Intl.Segmenter === "undefined"
    ? Array.from(value)
    : Array.from(new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(value), (part) => part.segment);

const group = (value: string) => {
  let offset = 0;
  return value
    .split(/(\s+)/)
    .filter(Boolean)
    .map((word) => {
      const glyphs = /^\s+$/.test(word) ? [] : segment(word);
      const item = { word, glyphs, offset, space: glyphs.length === 0 };
      offset += glyphs.length;
      return item;
    });
};

const hash = (n: number) => {
  const x = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return x - Math.floor(x);
};

const poolFor = (glyph: string, charset: string) => {
  if (charset && charset !== "auto") return Array.from(new Set(segment(charset)));
  if (/^[A-Z]$/.test(glyph)) return Array.from(UPPER);
  if (/^[a-z]$/.test(glyph)) return Array.from(LOWER);
  if (/^[0-9]$/.test(glyph)) return Array.from(DIGITS);
  if (/^\p{Lu}$/u.test(glyph)) return Array.from(UPPER);
  if (/^\p{L}$/u.test(glyph)) return Array.from(LOWER);
  return Array.from(SYMBOLS);
};

let colorContext: CanvasRenderingContext2D | null = null;
let widthContext: CanvasRenderingContext2D | null = null;
const widthCache = new Map<string, number>();

const glyphWidth = (glyph: string, font: string) => {
  const key = `${font}|${glyph}`;
  const cached = widthCache.get(key);
  if (cached !== undefined) return cached;
  if (!widthContext) widthContext = document.createElement("canvas").getContext("2d");
  if (!widthContext) return 1;
  widthContext.font = font;
  const width = widthContext.measureText(glyph).width;
  if (widthCache.size > 4000) widthCache.clear();
  widthCache.set(key, width);
  return width;
};

const matchWidth = (target: string, pool: string[], font: string) => {
  const goal = Math.max(glyphWidth(target, font), 1);
  const ranked = pool
    .filter((glyph) => glyph !== target)
    .map((glyph) => ({ glyph, gap: Math.abs(Math.log(Math.max(glyphWidth(glyph, font), 1) / goal)) }))
    .sort((a, b) => a.gap - b.gap);
  const close = ranked.filter((item) => item.gap < 0.28).map((item) => item.glyph);
  return close.length >= 5 ? close : ranked.slice(0, Math.min(6, ranked.length)).map((item) => item.glyph);
};

const parseColor = (value: string): [number, number, number] | null => {
  const direct = value.match(/^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/i);
  if (direct) return [Number(direct[1]) / 255, Number(direct[2]) / 255, Number(direct[3]) / 255];
  if (!colorContext) colorContext = document.createElement("canvas").getContext("2d");
  if (!colorContext) return null;
  colorContext.fillStyle = "#010203";
  colorContext.fillStyle = value;
  const read = String(colorContext.fillStyle);
  if (read === "#010203" && value.trim().toLowerCase() !== "#010203") return null;
  if (read.startsWith("#")) {
    const hex = read.slice(1);
    return [
      parseInt(hex.slice(0, 2), 16) / 255,
      parseInt(hex.slice(2, 4), 16) / 255,
      parseInt(hex.slice(4, 6), 16) / 255,
    ];
  }
  const parts = read.match(/[\d.]+/g);
  if (!parts || parts.length < 3) return null;
  return [Number(parts[0]) / 255, Number(parts[1]) / 255, Number(parts[2]) / 255];
};

const toLinear = (value: number) => (value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4));
const toGamma = (value: number) => (value <= 0.0031308 ? value * 12.92 : 1.055 * Math.pow(value, 1 / 2.4) - 0.055);

const toOklab = ([r, g, b]: [number, number, number]): [number, number, number] => {
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

const fromOklab = ([L, a, b]: [number, number, number]) => {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const rgb = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((value) => Math.round(clamp(toGamma(clamp(value, 0, 1)), 0, 1) * 255));
  return `rgb(${rgb[0]}, ${rgb[1]}, ${rgb[2]})`;
};

const createLottery = (root: HTMLElement, filterBase: string, settingsRef: { current: Settings }): Controller => {
  let reels: Reel[] = [];
  let raf = 0;
  let last = 0;
  let destroyed = false;
  let started = false;
  let visible = true;
  let base: [number, number, number] | null = null;
  let accent: [number, number, number] | null = null;
  let loopTimer = 0;
  let pending = false;
  let lineHeight = 1;
  const drags = new Map<number, Reel>();

  const colors = () => {
    const own = parseColor(getComputedStyle(root).color);
    base = own ? toOklab(own) : null;
    const tint = parseColor(settingsRef.current.accentColor);
    accent = tint ? toOklab(tint) : null;
  };

  const glyphAt = (reel: Reel, index: number) => {
    if (index === reel.goal && reel.mode !== "drag" && reel.mode !== "coast") return reel.target;
    const pick = Math.floor(hash(reel.seed * 91.7 + index * 13.13) * reel.pool.length);
    const glyph = reel.pool[pick] ?? reel.target;
    return glyph === reel.target && reel.pool.length > 1 ? reel.pool[(pick + 1) % reel.pool.length] : glyph;
  };

  const paint = (reel: Reel) => {
    const settings = settingsRef.current;
    const whole = Math.floor(reel.pos);
    const frac = reel.pos - whole;
    if (whole !== reel.shown || reel.mode === "rest") {
      reel.shown = whole;
      reel.cells[0].textContent = reel.mode === "rest" ? "" : glyphAt(reel, whole - 1);
      reel.cells[1].textContent = reel.mode === "rest" ? reel.target : glyphAt(reel, whole);
      reel.cells[2].textContent = reel.mode === "rest" ? "" : glyphAt(reel, whole + 1);
    }
    if (reel.mode === "rest") {
      reel.strip.style.transform = "";
      reel.root.style.filter = "";
      reel.root.style.removeProperty("mask-image");
      reel.root.style.removeProperty("-webkit-mask-image");
    } else {
      reel.strip.style.transform = `translate3d(0, ${(-frac * lineHeight).toFixed(3)}em, 0)`;
      const speed = Math.abs(reel.vel);
      const amount = clamp(settings.blur, 0, 1) * clamp((speed - 3) / 22, 0, 1);
      const level = amount <= 0.02 ? -1 : Math.min(BLUR_LEVELS.length - 1, Math.floor(amount * BLUR_LEVELS.length));
      reel.root.style.filter = level < 0 ? "" : `url(#${filterBase}-${level})`;
      const fade = clamp(settings.fade, 0, 1);
      const edge = fade > 0.01 ? `linear-gradient(to bottom, transparent 0%, #000 ${(fade * 24).toFixed(1)}%, #000 ${(100 - fade * 24).toFixed(1)}%, transparent 100%)` : "";
      if (edge) {
        reel.root.style.setProperty("mask-image", edge);
        reel.root.style.setProperty("-webkit-mask-image", edge);
      } else {
        reel.root.style.removeProperty("mask-image");
        reel.root.style.removeProperty("-webkit-mask-image");
      }
    }
    if (base && accent && reel.glow > 0.004) {
      const t = reel.glow;
      reel.root.style.color = fromOklab([
        base[0] + (accent[0] - base[0]) * t,
        base[1] + (accent[1] - base[1]) * t,
        base[2] + (accent[2] - base[2]) * t,
      ]);
    } else if (reel.root.style.color) {
      reel.root.style.color = "";
    }
  };

  const orderFor = (index: number, count: number) => {
    const direction = settingsRef.current.direction;
    const u = count > 1 ? index / (count - 1) : 0;
    if (direction === "right") return 1 - u;
    if (direction === "center") return Math.abs(u - 0.5) * 2;
    if (direction === "edges") return 1 - Math.abs(u - 0.5) * 2;
    if (direction === "random") return hash(index * 7.31 + 3.7);
    return u;
  };

  const measure = () => {
    const elements = Array.from(root.querySelectorAll<HTMLElement>("[data-reel]"));
    let rebuilt = false;
    const settings = settingsRef.current;
    const computed = getComputedStyle(root);
    const font = `${computed.fontStyle} ${computed.fontWeight} ${computed.fontSize} ${computed.fontFamily}`;
    const choose = (target: string) => {
      const pool = poolFor(target, settings.charset);
      return settings.charset && settings.charset !== "auto" ? pool : matchWidth(target, pool, font);
    };
    lineHeight = parseFloat(root.dataset.lineHeight ?? "1.1") || 1.1;
    const same = elements.length === reels.length && elements.every((el, i) => reels[i]?.root === el);
    if (!same) {
      rebuilt = true;
      const previous = reels;
      reels = elements.map((el, i) => {
        const strip = el.querySelector<HTMLElement>("[data-strip]")!;
        const cells = Array.from(strip.children) as HTMLElement[];
        const target = el.dataset.glyph ?? "";
        const old = previous[i];
        return {
          root: el,
          strip,
          cells,
          target,
          pool: choose(target),
          seed: hash(i * 3.17 + 0.5) * 1000,
          pos: old?.pos ?? 0,
          vel: 0,
          goal: 0,
          stopAt: 0,
          mode: "rest",
          shown: Number.NaN,
          glow: 0,
          order: orderFor(i, elements.length),
          dragY: 0,
          dragPos: 0,
          history: [],
        };
      });
      reels.forEach((reel) => {
        reel.pos = Math.round(reel.pos);
        reel.goal = Math.round(reel.pos);
        paint(reel);
      });
    } else {
      reels.forEach((reel, i) => {
        reel.pool = choose(reel.target);
        reel.order = orderFor(i, reels.length);
      });
    }
    colors();
    return rebuilt;
  };

  const roll = (all = true) => {
    const settings = settingsRef.current;
    if (settings.reduced || reels.length === 0) return;
    started = true;
    const now = performance.now() / 1000;
    const duration = clamp(settings.duration, 0, 10);
    const stagger = clamp(settings.stagger, 0, 1) * Math.max(reels.length - 1, 0);
    reels.forEach((reel) => {
      if (!all && reel.mode !== "rest") return;
      if (reel.mode === "drag") return;
      reel.mode = "spin";
      reel.stopAt = now + duration + reel.order * stagger;
      reel.seed = Math.random() * 1000;
      reel.shown = Number.NaN;
    });
    wake();
  };

  const settle = (reel: Reel) => {
    const settings = settingsRef.current;
    const speed = Math.max(Math.abs(reel.vel), 1);
    const reach = Math.max(1, Math.round(speed * 0.09 + 0.6 * clamp(settings.bounce, 0, 1)));
    reel.goal = Math.floor(reel.pos) + Math.sign(reel.vel || 1) * reach + (reel.vel < 0 ? 0 : 1);
    reel.mode = "settle";
    reel.shown = Number.NaN;
  };

  const step = (dt: number) => {
    const settings = settingsRef.current;
    const now = performance.now() / 1000;
    const top = clamp(settings.spinSpeed, 2, 60);
    const zeta = 0.95 - 0.7 * clamp(settings.bounce, 0, 1);
    const k = 170;
    const c = 2 * zeta * Math.sqrt(k);
    let busy = false;
    let landed = false;
    for (const reel of reels) {
      if (reel.mode === "spin") {
        reel.vel += (top - reel.vel) * (1 - Math.exp(-dt / 0.12));
        reel.pos += reel.vel * dt;
        if (now >= reel.stopAt) settle(reel);
        busy = true;
      } else if (reel.mode === "coast") {
        reel.vel *= Math.exp(-dt / 0.45);
        reel.pos += reel.vel * dt;
        if (Math.abs(reel.vel) <= 16) settle(reel);
        busy = true;
      } else if (reel.mode === "settle") {
        const sub = 4;
        const h = dt / sub;
        for (let i = 0; i < sub; i++) {
          const acc = -k * (reel.pos - reel.goal) - c * reel.vel;
          reel.vel += acc * h;
          reel.pos += reel.vel * h;
        }
        if (Math.abs(reel.pos - reel.goal) < 0.002 && Math.abs(reel.vel) < 0.05) {
          reel.pos = reel.goal;
          reel.vel = 0;
          reel.mode = "rest";
          reel.shown = Number.NaN;
          reel.glow = clamp(settings.flash, 0, 1);
          landed = true;
        }
        busy = true;
      } else if (reel.mode === "drag") {
        busy = true;
      }
      if (reel.glow > 0) {
        reel.glow = Math.max(0, reel.glow - dt / 0.7);
        busy = true;
      }
      paint(reel);
    }
    if (landed && reels.every((reel) => reel.mode === "rest")) {
      settingsRef.current.onComplete?.();
      schedule();
    }
    return busy;
  };

  const tick = (now: number) => {
    raf = 0;
    if (destroyed) return;
    const dt = Math.min(0.05, Math.max(0, (now - last) / 1000));
    last = now;
    const busy = step(dt);
    if (busy && !document.hidden) raf = requestAnimationFrame(tick);
  };

  function wake() {
    if (destroyed || raf) return;
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }

  function schedule() {
    window.clearTimeout(loopTimer);
    const settings = settingsRef.current;
    if (!settings.loop || settings.reduced) return;
    loopTimer = window.setTimeout(() => {
      if (!visible) {
        pending = true;
        return;
      }
      roll();
    }, clamp(settings.interval, 0.5, 60) * 1000);
  }

  const reelAt = (target: EventTarget | null) => {
    const el = (target as Element | null)?.closest?.("[data-reel]") as HTMLElement | null;
    return el ? reels.find((reel) => reel.root === el) ?? null : null;
  };

  const onDown = (event: PointerEvent) => {
    const settings = settingsRef.current;
    if (settings.reduced || event.pointerType === "touch" || (event.pointerType === "mouse" && event.button !== 0)) return;
    const reel = reelAt(event.target);
    if (!reel) return;
    if (settings.flickable) {
      event.preventDefault();
      try {
        root.setPointerCapture(event.pointerId);
      } catch {}
      reel.mode = "drag";
      reel.vel = 0;
      reel.dragY = event.clientY;
      reel.dragPos = reel.pos;
      reel.history = [{ t: performance.now(), pos: reel.pos }];
      reel.shown = Number.NaN;
      drags.set(event.pointerId, reel);
      wake();
    }
  };

  const onMove = (event: PointerEvent) => {
    const reel = drags.get(event.pointerId);
    if (!reel) return;
    const size = reel.root.getBoundingClientRect().height || 1;
    reel.pos = reel.dragPos + (reel.dragY - event.clientY) / size;
    const now = performance.now();
    reel.history.push({ t: now, pos: reel.pos });
    if (reel.history.length > 10) reel.history.shift();
    paint(reel);
  };

  const onUp = (event: PointerEvent) => {
    const reel = drags.get(event.pointerId);
    if (!reel) return;
    drags.delete(event.pointerId);
    try {
      root.releasePointerCapture(event.pointerId);
    } catch {}
    const now = performance.now();
    const recent = reel.history.filter((item) => now - item.t < 90);
    const moved = Math.abs(reel.pos - reel.dragPos);
    let velocity = 0;
    if (recent.length >= 2) {
      const span = Math.max((recent[recent.length - 1].t - recent[0].t) / 1000, 1 / 120);
      velocity = (recent[recent.length - 1].pos - recent[0].pos) / span;
    }
    if (moved < 0.08 && Math.abs(velocity) < 1) {
      reel.mode = "rest";
      reel.pos = Math.round(reel.pos);
      reel.goal = reel.pos;
      reel.shown = Number.NaN;
      paint(reel);
      if (settingsRef.current.clickToSpin) roll();
      return;
    }
    reel.vel = clamp(velocity, -60, 60);
    if (Math.abs(reel.vel) > 16) {
      reel.mode = "coast";
      reel.shown = Number.NaN;
    } else settle(reel);
    wake();
  };

  const onClick = (event: MouseEvent) => {
    const settings = settingsRef.current;
    if (!settings.clickToSpin || settings.reduced) return;
    const touch = (event as PointerEvent).pointerType === "touch";
    if (settings.flickable && !touch) return;
    if (!reelAt(event.target)) return;
    roll();
  };

  const onEnter = (event: PointerEvent) => {
    const settings = settingsRef.current;
    if (!settings.spinOnHover || settings.reduced || event.pointerType === "touch") return;
    if (reels.some((reel) => reel.mode !== "rest")) return;
    roll();
  };

  root.addEventListener("pointerdown", onDown);
  root.addEventListener("pointermove", onMove);
  root.addEventListener("pointerup", onUp);
  root.addEventListener("pointercancel", onUp);
  root.addEventListener("click", onClick);
  root.addEventListener("pointerenter", onEnter);
  const intersection = new IntersectionObserver(
    (entries) => {
      visible = entries.some((entry) => entry.isIntersecting);
      const entry = entries[0];
      const settings = settingsRef.current;
      if (!started && settings.trigger === "view" && entry && entry.intersectionRatio >= 0.4) roll();
      if (visible && pending) {
        pending = false;
        roll();
      }
    },
    { threshold: [0, 0.4, 1] },
  );
  intersection.observe(root);
  if (document.fonts?.ready)
    document.fonts.ready
      .then(() => {
        if (destroyed) return;
        widthCache.clear();
        measure();
      })
      .catch(() => {});
  measure();
  if (settingsRef.current.trigger === "mount") roll();

  return {
    measure: () => {
      if (measure() && started) roll();
    },
    sync: () => {
      colors();
      if (settingsRef.current.loop) {
        if (!loopTimer && reels.every((reel) => reel.mode === "rest")) schedule();
      } else {
        window.clearTimeout(loopTimer);
        loopTimer = 0;
      }
    },
    destroy: () => {
      destroyed = true;
      cancelAnimationFrame(raf);
      window.clearTimeout(loopTimer);
      root.removeEventListener("pointerdown", onDown);
      root.removeEventListener("pointermove", onMove);
      root.removeEventListener("pointerup", onUp);
      root.removeEventListener("pointercancel", onUp);
      root.removeEventListener("click", onClick);
      root.removeEventListener("pointerenter", onEnter);
      intersection.disconnect();
    },
    spin: () => roll(),
  };
};

const hidden: CSSProperties = {
  position: "absolute",
  width: 1,
  height: 1,
  padding: 0,
  margin: -1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
  border: 0,
};

const GlyphLottery = forwardRef<GlyphLotteryHandle, GlyphLotteryProps>(function GlyphLottery(
  {
    text = "Your lucky number",
    as = "span",
    fontSize = "clamp(2.75rem, 8vw, 6.5rem)",
    fontWeight = 700,
    letterSpacing = "-0.03em",
    lineHeight = 1.1,
    align = "center",
    color,
    accentColor = "#B15CFF",
    charset = "auto",
    trigger = "view",
    direction = "left",
    spinSpeed = 22,
    duration = 0.5,
    stagger = 0.06,
    bounce = 0.45,
    blur = 0.6,
    fade = 0.7,
    flash = 0.6,
    loop = false,
    interval = 4,
    spinOnHover = false,
    clickToSpin = true,
    flickable = true,
    onComplete,
    className,
    style,
  },
  ref,
) {
  const Tag = as as "span";
  const rootRef = useRef<HTMLElement>(null);
  const controllerRef = useRef<Controller | null>(null);
  const filterBase = `glyph-lottery-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const reduced = useSyncExternalStore(subscribeToMotion, readMotion, () => false);
  const groups = useMemo(() => group(text), [text]);
  const settingsRef = useRef<Settings>({
    accentColor,
    charset,
    trigger,
    direction,
    spinSpeed,
    duration,
    stagger,
    bounce,
    blur,
    fade,
    flash,
    loop,
    interval,
    spinOnHover,
    clickToSpin,
    flickable,
    reduced,
    onComplete,
  });

  useEffect(() => {
    settingsRef.current = {
      accentColor,
      charset,
      trigger,
      direction,
      spinSpeed,
      duration,
      stagger,
      bounce,
      blur,
      fade,
      flash,
      loop,
      interval,
      spinOnHover,
      clickToSpin,
      flickable,
      reduced,
      onComplete,
    };
    controllerRef.current?.sync();
  });

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const controller = createLottery(root, filterBase, settingsRef);
    controllerRef.current = controller;
    return () => {
      controller.destroy();
      controllerRef.current = null;
    };
  }, [filterBase]);

  useEffect(() => {
    controllerRef.current?.measure();
  }, [groups, charset, direction, color, accentColor]);

  useImperativeHandle(
    ref,
    () => ({
      spin: () => controllerRef.current?.spin(),
    }),
    [],
  );

  const cell: CSSProperties = { display: "block", height: `${lineHeight}em`, lineHeight, textAlign: "center", whiteSpace: "pre" };

  return (
    <Tag
      ref={rootRef as RefObject<HTMLSpanElement>}
      data-line-height={lineHeight}
      className={cn("relative inline-block max-w-full", className)}
      style={{
        fontSize: typeof fontSize === "number" ? `${fontSize}px` : fontSize,
        fontWeight,
        letterSpacing,
        lineHeight,
        textAlign: align,
        color,
        whiteSpace: "pre-wrap",
        userSelect: "none",
        WebkitUserSelect: "none",
        ...style,
      }}
    >
      <svg width="0" height="0" aria-hidden="true" style={{ position: "absolute" }}>
        {BLUR_LEVELS.map((amount, index) => (
          <filter key={index} id={`${filterBase}-${index}`} x="-70%" y="-50%" width="240%" height="200%">
            <feGaussianBlur stdDeviation={`0 ${amount}`} />
          </filter>
        ))}
      </svg>
      <span style={hidden}>{text}</span>
      <span aria-hidden="true">
        {groups.map((item, groupIndex) =>
          item.space ? (
            <span key={groupIndex}>{item.word}</span>
          ) : (
            <span key={groupIndex} style={{ display: "inline-block", whiteSpace: "nowrap" }}>
              {item.glyphs.map((glyph, i) => (
                <span
                  key={`${item.offset + i}-${glyph}`}
                  data-reel
                  data-glyph={glyph}
                  style={{
                    position: "relative",
                    display: "inline-block",
                    height: `${lineHeight}em`,
                    verticalAlign: "top",
                    clipPath: "polygon(-60% 0, 160% 0, 160% 100%, -60% 100%)",
                    cursor: flickable && !reduced ? "ns-resize" : clickToSpin && !reduced ? "pointer" : undefined,
                  }}
                >
                  <span style={{ visibility: "hidden", whiteSpace: "pre" }}>{glyph}</span>
                  <span
                    data-strip
                    style={{ position: "absolute", left: "-60%", right: "-60%", top: `-${lineHeight}em`, willChange: "transform" }}
                  >
                    <span style={cell} />
                    <span style={cell}>{glyph}</span>
                    <span style={cell} />
                  </span>
                </span>
              ))}
            </span>
          ),
        )}
      </span>
    </Tag>
  );
});

GlyphLottery.displayName = "GlyphLottery";

export { GlyphLottery };
export default GlyphLottery;
