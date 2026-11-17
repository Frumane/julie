// uBlock Origin'in betiklerini (scriptlet) ve yönlendirme kaynaklarını
// adblock-rust'ın `Resource` biçimine çevirir.
//
// Kullanım:
//   git clone --depth 1 --branch <sürüm> https://github.com/gorhill/uBlock.git /tmp/uBlock
//   node tools/build-ubo-resources.mjs /tmp/uBlock
//
// Çıktı: resources/ubo-resources.json (GPLv3, uBlock Origin'den türetilmiştir)

import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const uboDir = resolve(process.argv[2] ?? '');
const outFile = resolve(import.meta.dirname, '..', 'resources', 'ubo-resources.json');

// Güvenilir listelerin (uBlock'un kendi listeleri) kullanabildiği betikler
// bu izin bitini taşır. Julie'de aynı bit uBlock listelerine verilir.
const TRUSTED = 1;

const mimeByExtension = {
    js: 'application/javascript',
    fn: 'fn/javascript',
    css: 'text/css',
    html: 'text/html',
    json: 'application/json',
    txt: 'text/plain',
    xml: 'text/xml',
    gif: 'image/gif',
    png: 'image/png',
    mp3: 'audio/mp3',
    mp4: 'video/mp4',
};
const mimeOf = name => mimeByExtension[/\.([^.]+)$/.exec(name)?.[1]] ?? 'text/plain';
const base64 = data => Buffer.from(data).toString('base64');
const removeTopCommentBlock = text => text.replace(/^\/\*[\S\s]+?\*\/\s*/g, '');
const asList = value => value === undefined ? [] : Array.isArray(value) ? value : [ value ];

const version = execFileSync('git', [ '-C', uboDir, 'describe', '--tags', '--always' ]).toString().trim();
const resources = [];

// 1. Betikler: uBlock'un kendisinin yaptığı gibi modülü yükleyip
//    fonksiyonların kaynak kodunu alıyoruz.
const scriptletsModule = pathToFileURL(join(uboDir, 'src/js/resources/scriptlets.js'));
const { builtinScriptlets } = await import(scriptletsModule.href);
for ( const s of builtinScriptlets ) {
    const resource = {
        name: s.name,
        aliases: asList(s.aliases),
        kind: { mime: mimeOf(s.name) },
        content: base64(s.fn.toString()),
    };
    if ( s.dependencies?.length ) { resource.dependencies = s.dependencies; }
    if ( s.requiresTrust ) { resource.permission = TRUSTED; }
    resources.push(resource);
}
const scriptletCount = resources.length;

// 2. Yönlendirme kaynakları: engellenen bir isteğin yerine verilen
//    zararsız sahte dosyalar (boş betik, 1x1 resim, sahte Google Analytics...).
const redirectsModule = pathToFileURL(join(uboDir, 'src/js/redirect-resources.js'));
const { default: redirectable } = await import(redirectsModule.href);
const warDir = join(uboDir, 'src/web_accessible_resources');
const warFiles = new Set(readdirSync(warDir));
for ( const [ name, details ] of redirectable ) {
    if ( !warFiles.has(name) ) { continue; }
    const mime = mimeOf(name);
    let data = readFileSync(join(warDir, name));
    if ( mime.startsWith('text/') || mime === 'application/javascript' ) {
        data = removeTopCommentBlock(data.toString('utf8'));
    }
    resources.push({
        name,
        aliases: asList(details.alias),
        kind: { mime },
        content: base64(data),
    });
}

writeFileSync(outFile, JSON.stringify({
    source: 'https://github.com/gorhill/uBlock',
    version,
    license: 'GPL-3.0-or-later',
    resources,
}));
console.log(`uBlock ${version}: ${scriptletCount} betik, ${resources.length - scriptletCount} yönlendirme kaynağı -> ${outFile}`);
