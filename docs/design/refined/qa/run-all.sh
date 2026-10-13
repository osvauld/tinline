#!/bin/sh
set -eu
# Repeatable smoke run. Needs the static server: (cd docs/design && python3 -m http.server 8768 --bind 127.0.0.1)
# Uses /home/abe/project/sveditorjs/node_modules/playwright + /usr/sbin/chromium (see lib.js).
cd "$(dirname "$0")" || exit 1
mkdir -p screenshots
node baseline.js
node shots.js
node links.js
for f in calls chat groups privacy; do node flow-$f.js; done
node home-test.js
node - <<'JS'
const fs = require('fs');
const results = ['calls', 'chat', 'groups', 'privacy', 'home'].flatMap(name => JSON.parse(fs.readFileSync(`${name}-results.json`, 'utf8')));
const failed = results.filter(result => !result.ok);
console.log(`${results.length - failed.length}/${results.length} checks passed`);
if (failed.length) { console.error(failed); process.exit(1); }
JS
