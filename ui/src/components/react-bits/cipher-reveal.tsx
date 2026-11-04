
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useSyncExternalStore,
  type CSSProperties,
  type RefObject,
} from "react";
import { cn } from "@/lib/utils";

export type CipherRevealTag = "span" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6";
export type CipherRevealOrder = "left" | "right" | "center" | "random";
export type CipherRevealTrigger = "view" | "mount" | "manual";
export type CipherRevealHover = "trail" | "replay" | "none";

export interface CipherRevealHandle {
  replay: () => void;
}

export interface CipherRevealProps {
  text?: string;
  as?: CipherRevealTag;
  fontSize?: number | string;
  fontWeight?: number;
  letterSpacing?: string;
  lineHeight?: number;
  align?: "left" | "center" | "right";
  color?: string;
  scrambleColor?: string;
  scrambleOpacity?: number;
  charset?: string;
  trigger?: CipherRevealTrigger;
  order?: CipherRevealOrder;
  from?: "blank" | "scrambled";
  duration?: number;
  scramble?: number;
  rate?: number;
  hover?: CipherRevealHover;
  hoverRadius?: number;
  loop?: boolean;
  interval?: number;
  onComplete?: () => void;
  className?: string;
  style?: CSSProperties;
}

interface Settings {
  scrambleColor: string;
  scrambleOpacity: number;
  charset: string;
  trigger: CipherRevealTrigger;
  order: CipherRevealOrder;
  from: "blank" | "scrambled";
  duration: number;
  scramble: number;
  rate: number;
  hover: CipherRevealHover;
  hoverRadius: number;
  loop: boolean;
  interval: number;
  reduced: boolean;
  onComplete?: () => void;
}

interface Cell {
  root: HTMLElement;
  face: HTMLElement;
  mask: HTMLElement;
  glyph: string;
  pool: string[];
  state: "plain" | "hidden" | "scramble";
  active: boolean;
  appear: number;
  lock: number;
  phase: number;
  step: number;
  seed: number;
  shown: string;
  cooldown: number;
  x: number;
  y: number;
  width: number;
}

interface Controller {
  measure: (replay: boolean) => void;
  sync: () => void;
  destroy: () => void;
  replay: () => void;
}

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

const poolFor = (glyph: string, charset: string, font: string) => {
  if (charset && charset !== "auto") return Array.from(new Set(segment(charset))).filter((item) => item !== glyph);
  const base = /^[A-Z]$/.test(glyph) || /^\p{Lu}$/u.test(glyph)
    ? UPPER
    : /^[0-9]$/.test(glyph)
      ? DIGITS
      : /^\p{L}$/u.test(glyph)
        ? LOWER
        : SYMBOLS;
  const goal = Math.max(glyphWidth(glyph, font), 1);
  const rank = (source: string) =>
    Array.from(new Set(Array.from(source)))
      .filter((item) => item !== glyph)
      .map((item) => ({ item, gap: Math.abs(Math.log(Math.max(glyphWidth(item, font), 1) / goal)) }))
      .sort((a, b) => a.gap - b.gap);
  const own = rank(base).filter((entry) => entry.gap < 0.2).map((entry) => entry.item);
  if (own.length >= 4) return own;
  const wide = rank(UPPER + LOWER + DIGITS);
  const near = wide.filter((entry) => entry.gap < 0.22).map((entry) => entry.item);
  return near.length >= 3 ? near : wide.slice(0, 4).map((entry) => entry.item);
};

const createCipher = (root: HTMLElement, settingsRef: { current: Settings }): Controller => {
  let cells: Cell[] = [];
  let raf = 0;
  let destroyed = false;
  let started = false;
  let visible = true;
  let running = false;
  let loopTimer = 0;
  let font = "";
  let em = 16;
  let spacing = 0;
  let lastPointer = { x: 0, y: 0, t: 0 };

  const tint = (cell: Cell) => {
    cell.mask.style.color = settingsRef.current.scrambleColor || "";
  };

  const hide = (cell: Cell) => {
    cell.state = "hidden";
    cell.face.style.transition = "none";
    cell.face.style.opacity = "0";
    cell.mask.style.transition = "none";
    cell.mask.style.opacity = "0";
    cell.mask.textContent = "";
    cell.shown = "";
  };

  const scramble = (cell: Cell) => {
    cell.state = "scramble";
    cell.face.style.transition = "none";
    cell.face.style.opacity = "0";
    cell.mask.style.transition = "none";
  };

  const settle = (cell: Cell, fade: boolean) => {
    cell.state = "plain";
    cell.active = false;
    if (!fade) {
      cell.face.style.transition = "none";
      cell.face.style.opacity = "1";
      cell.mask.style.transition = "none";
      cell.mask.style.opacity = "0";
      cell.mask.textContent = "";
      cell.shown = "";
      return;
    }
    cell.mask.textContent = cell.glyph;
    cell.mask.style.transform = "translateX(-50%)";
    cell.shown = cell.glyph;
    cell.face.style.transition = "opacity 200ms cubic-bezier(.2,.7,.2,1)";
    cell.mask.style.transition = "opacity 200ms cubic-bezier(.2,.7,.2,1)";
    cell.face.style.opacity = "1";
    cell.mask.style.opacity = "0";
  };

  const draw = (cell: Cell, glyph: string) => {
    if (glyph === cell.shown) return;
    cell.shown = glyph;
    cell.mask.textContent = glyph;
    const natural = Math.max(glyphWidth(glyph, font) + spacing, 1);
    const stretch = clamp(cell.width / natural, 0.55, 1.6);
    cell.mask.style.transform = Math.abs(stretch - 1) > 0.004 ? `translateX(-50%) scaleX(${stretch.toFixed(3)})` : "translateX(-50%)";
  };

  const flicker = (cell: Cell, now: number) => {
    const settings = settingsRef.current;
    const step = Math.floor((now + cell.phase) * clamp(settings.rate, 2, 60));
    if (step === cell.step && cell.shown) return;
    cell.step = step;
    if (cell.pool.length === 0) {
      draw(cell, cell.glyph);
      return;
    }
    let index = Math.floor(cell.seed + step * 5) % cell.pool.length;
    if (cell.pool[index] === cell.shown) index = (index + 1) % cell.pool.length;
    draw(cell, cell.pool[index]);
  };

  const measure = () => {
    const settings = settingsRef.current;
    const computed = getComputedStyle(root);
    font = `${computed.fontStyle} ${computed.fontWeight} ${computed.fontSize} ${computed.fontFamily}`;
    em = parseFloat(computed.fontSize) || 16;
    spacing = parseFloat(computed.letterSpacing) || 0;
    const rootRect = root.getBoundingClientRect();
    const elements = Array.from(root.querySelectorAll<HTMLElement>("[data-cipher]"));
    const previous = new Map(cells.map((cell) => [cell.root, cell]));
    cells = elements.map((el, i) => {
      const face = el.querySelector<HTMLElement>("[data-face]")!;
      const mask = el.querySelector<HTMLElement>("[data-mask]")!;
      const glyph = el.dataset.glyph ?? "";
      const rect = face.getBoundingClientRect();
      const old = previous.get(el);
      const cell: Cell = {
        root: el,
        face,
        mask,
        glyph,
        pool: poolFor(glyph, settings.charset, font),
        state: old?.state ?? "plain",
        active: old?.active ?? false,
        appear: old?.appear ?? 0,
        lock: old?.lock ?? 0,
        phase: old?.phase ?? hash(i * 3.7 + 1.3),
        step: -1,
        seed: old?.seed ?? Math.floor(hash(i * 9.1 + 4.2) * 997),
        shown: "",
        cooldown: old?.cooldown ?? 0,
        x: rect.left + rect.width / 2 - rootRect.left,
        y: rect.top + rect.height / 2 - rootRect.top,
        width: rect.width,
      };
      mask.style.lineHeight = `${rect.height}px`;
      tint(cell);
      return cell;
    });
    return elements.length;
  };

  const rank = () => {
    const settings = settingsRef.current;
    const count = cells.length;
    const order = cells.map((cell, i) => {
      const u = count > 1 ? i / (count - 1) : 0;
      if (settings.order === "right") return 1 - u;
      if (settings.order === "center") return Math.abs(u - 0.5) * 2;
      if (settings.order === "random") return hash(i * 7.31 + Math.random() * 0.001 + 3.7);
      return u;
    });
    const sorted = order.map((value, i) => ({ value, i })).sort((a, b) => a.value - b.value);
    const ranks = new Array<number>(count);
    sorted.forEach((item, position) => {
      ranks[item.i] = count > 1 ? position / (count - 1) : 0;
    });
    return ranks;
  };

  const reveal = () => {
    const settings = settingsRef.current;
    if (cells.length === 0) return;
    started = true;
    if (settings.reduced) {
      cells.forEach((cell) => settle(cell, false));
      return;
    }
    const now = performance.now() / 1000;
    const duration = clamp(settings.duration, 0.1, 20);
    const hold = clamp(settings.scramble, 0.05, 5);
    const ranks = rank();
    const blank = settings.from === "blank";
    cells.forEach((cell, i) => {
      const at = ranks[i] * Math.max(duration - hold, duration * 0.35);
      cell.active = true;
      cell.appear = blank ? now + at : now;
      cell.lock = now + at + hold;
      cell.step = -1;
      if (blank) hide(cell);
      else {
        scramble(cell);
        flicker(cell, now);
        cell.mask.style.opacity = String(clamp(settings.scrambleOpacity, 0, 1) * 0.55);
      }
    });
    running = true;
    wake();
  };

  const tick = (stamp: number) => {
    raf = 0;
    if (destroyed) return;
    const settings = settingsRef.current;
    const now = stamp / 1000;
    const strength = clamp(settings.scrambleOpacity, 0, 1);
    let busy = false;
    for (const cell of cells) {
      if (!cell.active) continue;
      if (now >= cell.lock) {
        settle(cell, true);
        continue;
      }
      busy = true;
      if (now < cell.appear) continue;
      if (cell.state !== "scramble") scramble(cell);
      flicker(cell, now);
      const progress = clamp((now - cell.appear) / Math.max(cell.lock - cell.appear, 0.01), 0, 1);
      cell.mask.style.opacity = (strength * (0.55 + 0.45 * progress)).toFixed(3);
    }
    if (busy && visible && !document.hidden) raf = requestAnimationFrame(tick);
    else if (!busy && running) {
      running = false;
      settingsRef.current.onComplete?.();
      schedule();
    }
  };

  function wake() {
    if (destroyed || raf) return;
    raf = requestAnimationFrame(tick);
  }

  function schedule() {
    window.clearTimeout(loopTimer);
    const settings = settingsRef.current;
    if (!settings.loop || settings.reduced) return;
    loopTimer = window.setTimeout(() => {
      if (visible) reveal();
      else started = false;
    }, clamp(settings.interval, 0.5, 60) * 1000);
  }

  const onMove = (event: PointerEvent) => {
    const settings = settingsRef.current;
    if (settings.hover !== "trail" || settings.reduced || event.pointerType === "touch") return;
    const rect = root.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const stamp = performance.now();
    const speed = Math.hypot(x - lastPointer.x, y - lastPointer.y) / Math.max(stamp - lastPointer.t, 1);
    lastPointer = { x, y, t: stamp };
    if (speed < 0.08) return;
    const now = stamp / 1000;
    const reach = clamp(settings.hoverRadius, 0.2, 6) * em;
    let woke = false;
    for (const cell of cells) {
      if (cell.active || cell.state !== "plain" || now < cell.cooldown) continue;
      const distance = Math.hypot((cell.x - x) / reach, (cell.y - y) / (reach * 0.9));
      if (distance > 1) continue;
      cell.active = true;
      cell.appear = now;
      cell.lock = now + 0.14 + 0.22 * (1 - distance);
      cell.cooldown = cell.lock + 0.4;
      cell.step = -1;
      scramble(cell);
      flicker(cell, now);
      cell.mask.style.opacity = (clamp(settings.scrambleOpacity, 0, 1) * 0.55).toFixed(3);
      woke = true;
    }
    if (woke) wake();
  };

  const onEnter = (event: PointerEvent) => {
    const settings = settingsRef.current;
    if (settings.hover !== "replay" || settings.reduced || event.pointerType === "touch") return;
    if (cells.some((cell) => cell.active)) return;
    reveal();
  };

  root.addEventListener("pointermove", onMove);
  root.addEventListener("pointerenter", onEnter);
  const resizeObserver = new ResizeObserver(() => measure());
  resizeObserver.observe(root);
  const intersection = new IntersectionObserver(
    (entries) => {
      visible = entries.some((entry) => entry.isIntersecting);
      const entry = entries[0];
      const settings = settingsRef.current;
      if (!started && settings.trigger === "view" && entry && entry.intersectionRatio >= 0.4) reveal();
      if (visible) wake();
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
  const initial = settingsRef.current;
  if (initial.reduced || initial.trigger === "manual") cells.forEach((cell) => settle(cell, false));
  else cells.forEach((cell) => hide(cell));
  if (!initial.reduced && initial.trigger === "mount") reveal();

  return {
    measure: (replay: boolean) => {
      measure();
      if (replay && started) reveal();
    },
    sync: () => {
      cells.forEach(tint);
      const settings = settingsRef.current;
      if (!settings.loop) {
        window.clearTimeout(loopTimer);
        loopTimer = 0;
      } else if (!loopTimer && !cells.some((cell) => cell.active)) schedule();
      if (settings.reduced) cells.forEach((cell) => settle(cell, false));
    },
    destroy: () => {
      destroyed = true;
      cancelAnimationFrame(raf);
      window.clearTimeout(loopTimer);
      root.removeEventListener("pointermove", onMove);
      root.removeEventListener("pointerenter", onEnter);
      resizeObserver.disconnect();
      intersection.disconnect();
      cells.forEach((cell) => settle(cell, false));
    },
    replay: () => reveal(),
  };
};

const CipherReveal = forwardRef<CipherRevealHandle, CipherRevealProps>(function CipherReveal(
  {
    text = "Ideas into reality",
    as = "span",
    fontSize = "clamp(2.75rem, 8vw, 6.5rem)",
    fontWeight = 600,
    letterSpacing = "-0.03em",
    lineHeight = 1.1,
    align = "center",
    color,
    scrambleColor = "",
    scrambleOpacity = 0.42,
    charset = "auto",
    trigger = "view",
    order = "left",
    from = "blank",
    duration = 1.4,
    scramble = 0.45,
    rate = 14,
    hover = "trail",
    hoverRadius = 0.6,
    loop = false,
    interval = 5,
    onComplete,
    className,
    style,
  },
  ref,
) {
  const Tag = as as "span";
  const rootRef = useRef<HTMLElement>(null);
  const controllerRef = useRef<Controller | null>(null);
  const reduced = useSyncExternalStore(subscribeToMotion, readMotion, () => false);
  const groups = useMemo(() => group(text), [text]);
  const settingsRef = useRef<Settings>({
    scrambleColor,
    scrambleOpacity,
    charset,
    trigger,
    order,
    from,
    duration,
    scramble,
    rate,
    hover,
    hoverRadius,
    loop,
    interval,
    reduced,
    onComplete,
  });

  useEffect(() => {
    settingsRef.current = {
      scrambleColor,
      scrambleOpacity,
      charset,
      trigger,
      order,
      from,
      duration,
      scramble,
      rate,
      hover,
      hoverRadius,
      loop,
      interval,
      reduced,
      onComplete,
    };
    controllerRef.current?.sync();
  });

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const controller = createCipher(root, settingsRef);
    controllerRef.current = controller;
    return () => {
      controller.destroy();
      controllerRef.current = null;
    };
  }, []);

  useEffect(() => {
    controllerRef.current?.measure(true);
  }, [groups, charset]);

  useEffect(() => {
    controllerRef.current?.measure(false);
  }, [fontSize, fontWeight, letterSpacing, lineHeight, align]);

  useImperativeHandle(
    ref,
    () => ({
      replay: () => controllerRef.current?.replay(),
    }),
    [],
  );

  return (
    <Tag
      ref={rootRef as RefObject<HTMLSpanElement>}
      className={cn("relative inline-block max-w-full", className)}
      style={{
        fontSize: typeof fontSize === "number" ? `${fontSize}px` : fontSize,
        fontWeight,
        letterSpacing,
        lineHeight,
        textAlign: align,
        color,
        whiteSpace: "pre-wrap",
        ...style,
      }}
    >
      {groups.map((item, groupIndex) =>
        item.space ? (
          <span key={groupIndex}>{item.word}</span>
        ) : (
          <span key={groupIndex} style={{ whiteSpace: "nowrap" }}>
            {item.glyphs.map((glyph, i) => (
              <span key={`${item.offset + i}-${glyph}`} data-cipher data-glyph={glyph} style={{ position: "relative" }}>
                <span data-face style={{ opacity: reduced || trigger === "manual" ? undefined : 0 }}>
                  {glyph}
                </span>
                <span
                  data-mask
                  aria-hidden="true"
                  style={{
                    position: "absolute",
                    left: "50%",
                    top: 0,
                    transform: "translateX(-50%)",
                    transformOrigin: "50% 50%",
                    whiteSpace: "pre",
                    opacity: 0,
                    pointerEvents: "none",
                  }}
                />
              </span>
            ))}
          </span>
        ),
      )}
    </Tag>
  );
});

CipherReveal.displayName = "CipherReveal";

export { CipherReveal };
export default CipherReveal;
