// Kozmetik filtreleme: reklamlardan kalan boş kutuları gizler.
//
// Her sayfanın başında çalışır. Sayfadaki sınıf ve kimlik adlarını toplayıp
// tarayıcıya sorar; tarayıcı gizlenmesi gereken CSS seçicilerini
// `__julieCosmetic.hide(...)` ile geri gönderir.
(() => {
  if (window.top !== window || !window.ipc) return;
  if (location.protocol !== "http:" && location.protocol !== "https:") return;

  let style = null;
  const attach = () => {
    if (style && !style.isConnected) (document.head || document.documentElement)?.append(style);
  };
  Object.defineProperty(window, "__julieCosmetic", {
    value: Object.freeze({
      hide(css) {
        if (!css) return;
        if (!style) {
          style = document.createElement("style");
          style.setAttribute("data-julie", "");
        }
        style.textContent += css;
        attach();
      },
    }),
  });

  window.ipc.postMessage("cosmetic-start");

  const seen = new Set();
  let classes = [];
  let ids = [];
  let timer = 0;

  const flush = () => {
    timer = 0;
    attach();
    if (classes.length === 0 && ids.length === 0) return;
    window.ipc.postMessage("cosmetic-seen:" + classes.join(" ") + "\n" + ids.join(" "));
    classes = [];
    ids = [];
  };
  const schedule = () => {
    if (!timer) timer = setTimeout(flush, 50);
  };
  const collect = (el) => {
    const id = el.id;
    if (typeof id === "string" && id && !seen.has("#" + id)) {
      seen.add("#" + id);
      ids.push(id);
    }
    const list = el.classList;
    if (!list) return;
    for (const c of list) {
      if (!seen.has("." + c)) {
        seen.add("." + c);
        classes.push(c);
      }
    }
  };
  const scan = (node) => {
    if (node.nodeType !== 1) return;
    collect(node);
    for (const el of node.querySelectorAll("[id],[class]")) collect(el);
  };

  const start = () => {
    scan(document.documentElement);
    schedule();
    new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.type === "attributes") collect(m.target);
        else for (const n of m.addedNodes) scan(n);
      }
      schedule();
    }).observe(document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["class", "id"],
    });
  };

  if (document.documentElement) start();
  else document.addEventListener("readystatechange", start, { once: true });
})();
