#!/usr/bin/env python3
"""Chrome'un açık kaynak geliştirici araçlarını (devtools-frontend,
BSD lisanslı) indirip Julie'nin gömdüğü pakete (assets/devtools.zip)
toplar. Google'ın sunduğu derlenmiş sürümden, verilen Chromium
sürümünün dosyaları alınır; içe aktarılan her dosya takip edilir.

Kullanım: python3 tools/fetch_devtools.py <chromium-commit> <npm-sürümü>
  chromium-commit: chromiumdash'teki "hashes.chromium"
  npm-sürümü: aynı sürümün chrome-devtools-frontend paketi (dosya listesi için)

Paket ayrıca `julie-events.txt` içerir: protokoldeki bütün olay adları
(köprü bunlara abone olur).
"""
import io, json, re, sys, tarfile, urllib.request, zipfile, posixpath
from concurrent.futures import ThreadPoolExecutor

BASE = "https://chrome-devtools-frontend.appspot.com/serve_rev/@{rev}/"
EXT = r"js|mjs|css|svg|json|wasm|png|avif|gif|woff2|html|map"
LITERAL = re.compile(r"""["'`]((?:\.{1,2}/|[A-Za-z_][\w-]*/)[\w./@-]+\.(?:%s))["'`]""" % EXT)
URLFN = re.compile(r"""url\(\s*["']?((?:\.{1,2}/)?[\w./@-]+\.(?:%s))["']?\s*\)""" % EXT)
TEXT = (".js", ".mjs", ".css", ".html", ".json")
LOCALES = ("en-US", "tr")


def get(url):
    try:
        with urllib.request.urlopen(url, timeout=60) as r:
            return r.read() if r.status == 200 else None
    except Exception:
        return None


def main():
    rev, npm_version = sys.argv[1], sys.argv[2]
    base = BASE.format(rev=rev)
    tgz = get(f"https://registry.npmjs.org/chrome-devtools-frontend/-/chrome-devtools-frontend-{npm_version}.tgz")
    tar = tarfile.open(fileobj=io.BytesIO(tgz))
    names = tar.getnames()
    # Lisans metinleri (araçların kendisi ve içindeki üçüncü taraf kodlar).
    licenses = {}
    for n in names:
        if posixpath.basename(n).upper().startswith(("LICENSE", "COPYING")) and (
                n.count("/") == 1 or n.startswith("package/front_end/third_party/")):
            licenses["licenses/" + n[len("package/"):].replace("front_end/third_party/", "")] = \
                tar.extractfile(n).read()
    src = [n[len("package/front_end/"):] for n in names if n.startswith("package/front_end/")]

    seeds = {"inspector.html", "devtools_app.html", "worker_app.html",
             "application_tokens.css", "design_system_tokens.css"}
    for n in src:
        d, f = posixpath.split(n)
        if n.startswith("Images/") and f.endswith((".svg", ".png", ".avif", ".gif")):
            seeds.add(n)
        if f.endswith(".ts") and not f.endswith((".test.ts", ".d.ts")) and "/third_party/" not in "/" + d + "/":
            stem = f[:-3]
            if stem == posixpath.basename(d) or d.startswith("entrypoints/"):
                seeds.add(posixpath.join(d, stem + ".js"))
    for loc in LOCALES:
        seeds.add(f"core/i18n/locales/{loc}.json")

    files, seen, queue = {}, set(), list(seeds)
    with ThreadPoolExecutor(32) as pool:
        while queue:
            batch = [p for p in dict.fromkeys(queue) if p not in seen]
            queue = []
            seen.update(batch)
            for path, data in zip(batch, pool.map(lambda p: get(base + p), batch)):
                if data is None:
                    continue
                files[path] = data
                if path.endswith(TEXT):
                    text = data.decode("utf-8", "replace")
                    for m in list(LITERAL.finditer(text)) + list(URLFN.finditer(text)):
                        ref = m.group(1)
                        joined = posixpath.normpath(posixpath.join(posixpath.dirname(path), ref)) \
                            if ref.startswith(".") else ref
                        if not joined.startswith("..") and joined not in seen:
                            queue.append(joined)
            print(f"{len(files)} dosya, sırada {len(set(queue) - seen)}", file=sys.stderr)

    events = sorted({m.group(1) for data in files.values()
                     for m in re.finditer(rb"registerEvent\(\s*['\"]([A-Za-z]+\.[A-Za-z]+)['\"]", data)})
    events = [e.decode() for e in events]
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for path in sorted(files):
            z.writestr(path, files[path])
        for path in sorted(licenses):
            z.writestr(path, licenses[path])
        z.writestr("julie-events.txt", "\n".join(events) + "\n")
        z.writestr("julie-version.txt", f"chromium {rev}\nnpm {npm_version}\n")
    open("assets/devtools.zip", "wb").write(out.getvalue())
    total = sum(len(v) for v in files.values())
    print(f"{len(files)} dosya, {total/1e6:.1f} MB -> assets/devtools.zip "
          f"{len(out.getvalue())/1e6:.1f} MB, {len(events)} olay", file=sys.stderr)


if __name__ == "__main__":
    main()
