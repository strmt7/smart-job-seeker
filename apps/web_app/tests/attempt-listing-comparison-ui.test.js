const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync('apps/web_app/web/app.js','utf8');
const line=source.split('\n').find(item=>item.startsWith('function listingSnapshotNotice('));assert.ok(line);
const notice=vm.runInNewContext(`(${line})`);
assert.match(notice('matches'),/current workspace listing.*live vacancy may have changed/);
assert.match(notice('changed'),/differs.*saved listing/);
assert.match(notice('unavailable'),/could not be compared/);
assert.match(notice(undefined),/could not be compared/);
console.log('Listing snapshot comparison UI checks passed.');
