# resources/

## ubo-resources.json

[uBlock Origin](https://github.com/gorhill/uBlock)'in betikleri (scriptlet) ve
yönlendirme kaynakları. uBlock Origin'den türetilmiştir ve
**GPL-3.0-or-later** lisanslıdır (Copyright (C) Raymond Hill ve katkıda
bulunanlar). Hangi uBlock sürümünden üretildiği dosyanın `version` alanında
yazar.

Yeniden üretmek (ör. yeni bir uBlock sürümüne geçmek) için:

```bash
git clone --depth 1 --branch <sürüm> https://github.com/gorhill/uBlock.git /tmp/uBlock
```

```bash
node tools/build-ubo-resources.mjs /tmp/uBlock
```
