// Collect layout data for navbar/sidebar/footer detection.
(() => {
  const sel =
    "div,section,article,main,aside,nav,header,footer," +
    "p,h1,h2,h3,h4,h5,h6,ul,ol,table,pre,blockquote,figure";

  // Minimum number of elements before we consider the page well-structured
  // enough to skip the Shadow DOM fallback.
  const MIN_ELEMENTS = 5;

  const result = [];

  for (const el of document.querySelectorAll(sel)) {
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden") continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    result.push({
      tag: el.tagName,
      role: el.getAttribute("role"),
      w: r.width,
      h: r.height,
      position: cs.position,
    });
  }

  // Fallback: query inside open shadow roots for web-component-heavy pages.
  // Note: closed shadow roots (attachShadow({mode: "closed"})) are not
  // accessible via el.shadowRoot and cannot be queried.
  if (result.length < MIN_ELEMENTS) {
    for (const el of document.querySelectorAll("*")) {
      if (!el.shadowRoot) continue;
      for (const se of el.shadowRoot.querySelectorAll(sel)) {
        const cs = getComputedStyle(se);
        if (cs.display === "none" || cs.visibility === "hidden") continue;
        const r = se.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) continue;
        result.push({
          tag: se.tagName,
          role: se.getAttribute("role"),
          w: r.width,
          h: r.height,
          position: cs.position,
        });
      }
    }
  }

  return JSON.stringify(result);
})()
