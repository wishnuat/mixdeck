// Runs in the page's MAIN world at document_start, in every frame.
//
// Applies a per-tab gain on top of whatever volume the site itself sets:
//   real element volume = (volume the site asked for) × (Mixdeck tab gain)
// The `volume` property is intercepted so the site keeps reading back its own value. Its UI and saved
// preferences are never disturbed. Web Audio output is routed through a per-context GainNode.
(() => {
  if (window.__mixdeckGain) return;
  const state = { gain: 1 };
  window.__mixdeckGain = state;

  const clamp = (v) => Math.max(0, Math.min(1, v));

  // ---------------------------------------------------------------- <audio> / <video>
  const proto = HTMLMediaElement.prototype;
  const vol = Object.getOwnPropertyDescriptor(proto, "volume");
  const siteVolume = new WeakMap(); // element → volume the site asked for
  const media = new Set(); // WeakRef<HTMLMediaElement>

  const track = (el) => {
    if (!siteVolume.has(el)) {
      siteVolume.set(el, vol.get.call(el));
      media.add(new WeakRef(el));
    }
  };
  const apply = (el) => {
    track(el);
    const want = clamp(siteVolume.get(el) * state.gain);
    if (Math.abs(vol.get.call(el) - want) > 1e-4) vol.set.call(el, want);
  };

  Object.defineProperty(proto, "volume", {
    configurable: true,
    enumerable: vol.enumerable,
    get() {
      return siteVolume.has(this) ? siteVolume.get(this) : vol.get.call(this);
    },
    set(v) {
      const n = Number(v);
      if (!(n >= 0 && n <= 1)) return vol.set.call(this, v); // let the browser throw its usual error
      track(this);
      siteVolume.set(this, n);
      vol.set.call(this, clamp(n * state.gain));
    },
  });

  // Elements created with `new Audio()` are never in the DOM: catch them when they start playing.
  const play = proto.play;
  proto.play = function (...args) {
    apply(this);
    return play.apply(this, args);
  };
  // DOM elements (autoplay, controls): `play` doesn't bubble, but it does reach a capturing listener.
  document.addEventListener("play", (e) => e.target instanceof HTMLMediaElement && apply(e.target), true);

  // ---------------------------------------------------------------- Web Audio
  const gains = new Set(); // WeakRef<GainNode>
  const Base = window.BaseAudioContext || window.AudioContext;
  if (Base) {
    const dest = Object.getOwnPropertyDescriptor(Base.prototype, "destination");
    const perContext = new WeakMap();
    if (dest && dest.get) {
      Object.defineProperty(Base.prototype, "destination", {
        configurable: true,
        enumerable: dest.enumerable,
        get() {
          const real = dest.get.call(this);
          if (window.OfflineAudioContext && this instanceof OfflineAudioContext) return real; // rendering, not playback
          let g = perContext.get(this);
          if (!g) {
            g = new GainNode(this, { gain: state.gain });
            g.connect(real);
            // look like a destination node to code that inspects it
            for (const k of ["maxChannelCount"]) Object.defineProperty(g, k, { get: () => real[k] });
            perContext.set(this, g);
            gains.add(new WeakRef(g));
          }
          return g;
        },
      });
    }
  }

  // ---------------------------------------------------------------- updates from the extension
  const setGain = (g) => {
    if (typeof g !== "number" || !(g >= 0) || g > 1) return;
    state.gain = g;
    for (const ref of media) {
      const el = ref.deref();
      if (el) apply(el); else media.delete(ref);
    }
    for (const ref of gains) {
      const node = ref.deref();
      if (!node) { gains.delete(ref); continue; }
      try { node.gain.setTargetAtTime(g, node.context.currentTime, 0.015); } catch { node.gain.value = g; }
    }
  };
  document.addEventListener("mixdeck:gain", (e) => setGain(e.detail));
  document.dispatchEvent(new CustomEvent("mixdeck:ready"));
})();
