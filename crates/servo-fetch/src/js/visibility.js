// Reports CSS visibility signals AccessKit doesn't expose.

(() => {
  // Capture native APIs — page main world is exposed to prototype tampering.
  const _gcs = window.getComputedStyle.bind(window);
  const _qsa = Document.prototype.querySelectorAll;
  const _setAttr = Element.prototype.setAttribute;
  const _gbcr = Element.prototype.getBoundingClientRect;

  const SELECTOR =
    "div,section,article,main,aside,nav,header,footer," +
    "p,h1,h2,h3,h4,h5,h6,ul,ol,li,table,pre,blockquote,figure,span,a,button," +
    "details,dialog";

  const F_OPACITY_ZERO              = 1 << 4;
  const F_CLIPPED                   = 1 << 5;
  const F_CONTENT_VISIBILITY_HIDDEN = 1 << 6;
  const F_TEXT_INDENT_OFFSCREEN     = 1 << 7;
  const F_SR_ONLY                   = 1 << 8;
  const F_VISIBILITY_HIDDEN         = 1 << 9;

  const MAX = 5000;
  const reports = [];

  const isClipped = (cs) =>
    cs.clip === "rect(0px, 0px, 0px, 0px)" ||
    cs.clip === "rect(1px, 1px, 1px, 1px)" ||
    cs.clipPath === "inset(100%)" ||
    cs.clipPath === "inset(50%)" ||
    cs.clipPath === "circle(0px)";

  // Iterative to avoid stack overflow on deep DOM (10k+ nesting).
  const cumOpacity = new WeakMap();
  const cumOpacityOf = (el) => {
    if (cumOpacity.has(el)) return cumOpacity.get(el);
    const chain = [];
    let cur = el;
    while (cur && !cumOpacity.has(cur)) {
      chain.push(cur);
      cur = cur.parentElement;
    }
    let acc = cur ? cumOpacity.get(cur) : 1;
    for (let i = chain.length - 1; i >= 0; i--) {
      acc *= parseFloat(_gcs(chain[i]).opacity);
      cumOpacity.set(chain[i], acc);
    }
    return acc;
  };

  const looksSrOnly = (cs, r) => {
    if (r.width > 1.5 || r.height > 1.5) return false;
    return (isClipped(cs) || cs.overflow === "hidden") && cs.position === "absolute";
  };

  let nextId = 0;
  for (const el of _qsa.call(document, SELECTOR)) {
    if (++nextId > MAX) break;

    const cs = _gcs(el);
    const r = _gbcr.call(el);
    let flags = 0;

    // transform:scale(0) and display:none ancestors yield degenerate rects.
    if (cumOpacityOf(el) < 0.01 || (r.width < 0.5 && r.height < 0.5)) flags |= F_OPACITY_ZERO;
    if (cs.visibility === "hidden") flags |= F_VISIBILITY_HIDDEN;
    if (cs.contentVisibility === "hidden") flags |= F_CONTENT_VISIBILITY_HIDDEN;
    if (parseInt(cs.textIndent, 10) <= -9999) flags |= F_TEXT_INDENT_OFFSCREEN;
    if (looksSrOnly(cs, r)) {
      flags |= F_SR_ONLY;
    } else if (isClipped(cs)) {
      flags |= F_CLIPPED;
    }

    if (flags !== 0) {
      const id = String(nextId);
      _setAttr.call(el, "data-vf-id", id);
      reports.push({ id, flags });
    }
  }

  return JSON.stringify(reports);
})()
