// Sayfa çevirisi. Tarayıcı bu betiği "Türkçeye çevir" seçilince çalıştırır.
//
// Yazılar cümle cümle çevrilsin diye bir paragraftaki yazı ve içindeki
// bağlantı, kalın yazı gibi satır içi öğeler tek parça ("bölüm") olarak
// gönderilir: `Bir <b i="1">bağlantı</b> ile`. Gelen çeviri yine bu
// biçimdedir; her parça sayfadaki kendi yazı düğümüne yazılır. Düğümler
// yerinden oynatılmaz, yalnızca yazıları değişir; böylece sayfanın kendi
// betikleri (React vb.) bozulmaz.
//
// Paketler tarayıcıya `julie-translate:` iletisiyle gider, çeviriler
// `apply` ile gelir. Sonradan eklenen yazılar da çevrilir. `restore` her
// şeyi ilk haline döndürür.
(() => {
  if (window.__julieTranslate) {
    window.__julieTranslate.start();
    return;
  }
  const SKIP = new Set([
    "SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "TEXTAREA", "INPUT", "SELECT", "OPTION",
    "CODE", "PRE", "KBD", "SAMP", "VAR", "SVG", "MATH", "CANVAS", "IFRAME", "OBJECT",
  ]);
  const SKIP_SELECTOR =
    "script,style,noscript,template,textarea,input,select,option,code,pre,kbd,samp,var," +
    "svg,math,canvas,iframe,object,[translate=no],.notranslate,[contenteditable]";
  // Cümlenin içinde kalan öğeler.
  const INLINE = new Set([
    "A", "ABBR", "ACRONYM", "B", "BDI", "BDO", "BIG", "BUTTON", "CITE", "DEL", "DFN", "EM", "FONT",
    "I", "INS", "LABEL", "MARK", "NOBR", "Q", "S", "SMALL", "SPAN", "STRIKE", "STRONG",
    "SUB", "SUP", "TIME", "TT", "U",
  ]);
  const MAX_CHARS = 4000;
  const MAX_ITEMS = 100;
  // Harf içermeyen yazılar (sayılar, işaretler) çevrilmez.
  const LETTER = /\p{L}/u;

  const originals = new Map(); // yazı düğümü -> ilk hali
  const mine = new WeakMap(); // yazı düğümü -> bizim koyduğumuz çeviri
  const sent = new WeakSet(); // yolda olan düğümler
  const pending = new Map(); // paket no -> bölümler
  let next = 1;
  let active = false;
  let observer = null;
  let timer = 0;
  let dirty = [];

  const tag = (el) => el.tagName.toUpperCase();
  const skipped = (el) =>
    SKIP.has(tag(el)) ||
    el.isContentEditable ||
    el.getAttribute("translate") === "no" ||
    el.classList.contains("notranslate");

  // Yalnızca yazı ve satır içi öğelerden oluşan öğe.
  function inlineOnly(el) {
    if (!INLINE.has(tag(el)) || skipped(el)) return false;
    for (const child of el.childNodes) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        if (!inlineOnly(child)) return false;
      } else if (child.nodeType !== Node.TEXT_NODE && child.nodeType !== Node.COMMENT_NODE) {
        return false;
      }
    }
    return true;
  }

  const isText = (n) => n.nodeType === Node.TEXT_NODE;
  const fresh = (n) => isText(n) && LETTER.test(n.data) && !sent.has(n) && mine.get(n) !== n.data;

  function texts(nodes, out = []) {
    for (const n of nodes) {
      if (isText(n)) out.push(n);
      else if (n.nodeType === Node.ELEMENT_NODE) texts(n.childNodes, out);
    }
    return out;
  }

  const escape = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

  // Bölümün gönderilen hali; `ids` satır içi öğeleri numaralar.
  function markup(nodes, ids) {
    let out = "";
    for (const n of nodes) {
      if (isText(n)) out += escape(n.data);
      else if (n.nodeType === Node.ELEMENT_NODE) {
        ids.push(n);
        out += `<b i="${ids.length}">` + markup(n.childNodes, ids) + "</b>";
      }
    }
    return out;
  }

  // Bir öğenin içindeki bölümleri bulur.
  function scan(container, out) {
    let run = [];
    const flush = () => {
      if (run.length) out.push(run);
      run = [];
    };
    for (const child of container.childNodes) {
      if (isText(child) || (child.nodeType === Node.ELEMENT_NODE && inlineOnly(child))) {
        run.push(child);
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        flush();
        if (!skipped(child)) scan(child, out);
      }
    }
    flush();
  }

  // Bölümleri pakete hazırlar: çevrilecek yazısı olanlar, çok uzunsa
  // düğüm düğüm.
  function sections(runs) {
    const out = [];
    for (const nodes of runs) {
      const all = texts(nodes);
      if (!all.some(fresh)) continue;
      const ids = [];
      const text = markup(nodes, ids);
      if (text.length <= MAX_CHARS) {
        out.push({ nodes, ids, text, all });
        continue;
      }
      for (const n of all) {
        const one = escape(n.data);
        if (fresh(n) && one.length <= MAX_CHARS) {
          out.push({ nodes: [n], ids: [], text: one, all: [n] });
        }
      }
    }
    return out;
  }

  function inView(section) {
    const node = section.all[0];
    const el = node && node.parentElement;
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return r.bottom > 0 && r.top < innerHeight && r.width > 0 && r.height > 0;
  }

  function send(list) {
    // Ekranda görünenler önce çevrilsin.
    const seen = [];
    const rest = [];
    for (const s of list) (inView(s) ? seen : rest).push(s);
    let batch = [];
    let chars = 0;
    const flush = () => {
      if (!batch.length) return;
      const id = next++;
      pending.set(id, batch);
      const message = { batch: id, texts: batch.map((s) => s.text) };
      window.ipc.postMessage("julie-translate:" + JSON.stringify(message));
      batch = [];
      chars = 0;
    };
    for (const s of seen.concat(rest)) {
      if (batch.length >= MAX_ITEMS || chars + s.text.length > MAX_CHARS) flush();
      for (const n of s.all) sent.add(n);
      batch.push(s);
      chars += s.text.length;
    }
    flush();
  }

  const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };
  function decode(s) {
    return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (all, e) => {
      if (e[0] === "#") {
        const hex = e[1] === "x" || e[1] === "X";
        const code = hex ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
        return code > 0 && code < 0x110000 ? String.fromCodePoint(code) : all;
      }
      return ENTITIES[e.toLowerCase()] ?? all;
    });
  }

  // Gelen çeviriyi ağaca çevirir: yazılar ve {id, children}.
  function parse(html) {
    const root = { children: [] };
    const stack = [root];
    for (const m of html.matchAll(/<b\s+i\s*=\s*"?(\d+)"?\s*>|<\/b\s*>|[^<]+|</gi)) {
      const top = stack[stack.length - 1];
      if (m[1]) {
        const node = { id: Number(m[1]), children: [] };
        top.children.push(node);
        stack.push(node);
      } else if (/^<\/b/i.test(m[0])) {
        if (stack.length > 1) stack.pop();
      } else {
        top.children.push(decode(m[0]));
      }
    }
    return root.children;
  }

  function setText(node, value) {
    if (node.data === value) return;
    if (!originals.has(node)) originals.set(node, node.data);
    mine.set(node, value);
    node.data = value;
  }

  // Çeviriyi düğümlere dağıtır. Öğelerin arasındaki yazı aralıkları
  // ("yuvalar") sırayla çevirideki aralıklara karşılık gelir.
  function fill(nodes, children, ids) {
    const slots = [[]];
    for (const n of nodes) {
      if (isText(n)) slots[slots.length - 1].push(n);
      else if (n.nodeType === Node.ELEMENT_NODE) slots.push([]);
    }
    const runs = [""];
    for (const c of children) {
      if (typeof c === "string") runs[runs.length - 1] += c;
      else {
        runs.push("");
        const el = ids[c.id - 1];
        if (el) fill([...el.childNodes], c.children, ids);
      }
    }
    // Fazla aralıklar sonuncuya eklenir.
    if (runs.length > slots.length) {
      runs[slots.length - 1] = runs.slice(slots.length - 1).join(" ");
    }
    let carry = "";
    let last = null;
    slots.forEach((slot, i) => {
      const raw = (carry ? carry + " " : "") + (runs[i] ?? "");
      const text = raw.trim();
      carry = "";
      const words = slot.filter((n) => LETTER.test(n.data));
      const target = words[0] || slot[0];
      if (!target) {
        carry = text; // yazı düğümü yok: sonrakine kalsın
        return;
      }
      last = target;
      // Boşluklar sayfadakinden; sayfada yoksa çevirideki boşluk korunur
      // ("<b>hemen</b> devam" bitişmesin).
      const lead = target.data.match(/^\s*/)[0] || (/^\s/.test(raw) ? " " : "");
      const trail = target.data.match(/\s*$/)[0] || (/\s$/.test(raw) ? " " : "");
      if (text) setText(target, lead + text + trail);
      else if (words.length) setText(target, lead || trail);
      for (const n of words.slice(1)) setText(n, "");
    });
    if (carry && last) setText(last, last.data.replace(/\s*$/, (s) => " " + carry + s));
  }

  function apply(id, results) {
    const batch = pending.get(id);
    pending.delete(id);
    if (!batch) return;
    batch.forEach((s, i) => {
      for (const n of s.all) sent.delete(n);
      const done = results[i];
      if (!active || typeof done !== "string") return;
      // Bu arada sayfa bölümü değiştirdiyse dokunma.
      if (!s.nodes.every((n) => n.isConnected) || markup(s.nodes, []) !== s.text) return;
      fill(s.nodes, parse(done), s.ids);
    });
  }

  function fail(id) {
    const batch = pending.get(id);
    pending.delete(id);
    if (batch) for (const s of batch) for (const n of s.all) sent.delete(n);
  }

  // Değişen düğümün içinde bulunduğu blok (paragraf, liste öğesi …).
  function blockOf(node) {
    let el = isText(node) ? node.parentElement : node;
    while (el && el !== document.body && el.parentElement && INLINE.has(tag(el))) {
      el = el.parentElement;
    }
    return el;
  }

  function changed(records) {
    for (const r of records) {
      if (r.type === "characterData") {
        if (mine.get(r.target) !== r.target.data) dirty.push(r.target);
      } else {
        for (const node of r.addedNodes) dirty.push(node);
      }
    }
    if (!timer && dirty.length) {
      timer = setTimeout(() => {
        timer = 0;
        const blocks = new Set();
        for (const node of dirty) {
          const block = node.isConnected && blockOf(node);
          if (block && block.nodeType === Node.ELEMENT_NODE && !block.closest(SKIP_SELECTOR)) {
            blocks.add(block);
          }
        }
        dirty = [];
        const runs = [];
        for (const block of blocks) {
          // İç içe bloklar bir kez taransın.
          let parent = block.parentElement;
          while (parent && !blocks.has(parent)) parent = parent.parentElement;
          if (!parent) scan(block, runs);
        }
        const list = sections(runs);
        if (list.length && active) send(list);
      }, 400);
    }
  }

  function start() {
    if (active) return;
    active = true;
    const runs = [];
    scan(document.body, runs);
    send(sections(runs));
    observer = new MutationObserver(changed);
    observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  }

  function restore() {
    active = false;
    if (observer) observer.disconnect();
    observer = null;
    clearTimeout(timer);
    timer = 0;
    dirty = [];
    pending.clear();
    for (const [node, text] of originals) {
      if (mine.get(node) === node.data) node.data = text;
      mine.delete(node);
    }
    originals.clear();
  }

  window.__julieTranslate = { start, apply, fail, restore };
  start();
})();
