const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const start = source.indexOf('function setWorkspaceView(view){');
const end = source.indexOf('\nsetWorkspaceView(workspaceViewForHash(location.hash));', start);
const pane = { dataset: { view: 'documents' }, scrollTop: 450 };
const sections = new Map();
const location = { hash: '#settings-section' };
const context = vm.createContext({
  location,
  workspaceViewForHash: (hash) => (hash === '#settings-section' ? 'settings' : 'profile'),
  history: {
    replaceState(_state, _title, hash) {
      location.hash = hash;
    },
  },
  document: {
    querySelector: () => pane,
    querySelectorAll: () => [],
    getElementById: (id) => {
      if (!sections.has(id)) sections.set(id, {});
      return sections.get(id);
    },
  },
});
vm.runInContext(source.slice(start, end), context);
context.setWorkspaceView('settings');
assert.equal(pane.scrollTop, 0, 'opening another workspace section resets its old scroll');
assert.equal(sections.get('settings-section').hidden, false);
pane.scrollTop = 140;
context.setWorkspaceView('settings');
assert.equal(pane.scrollTop, 140, 'refreshing the current section preserves reading position');
context.setWorkspaceView('profile');
assert.equal(pane.scrollTop, 0);
console.log(
  'Workspace navigation resets stale scroll while preserving the current reading position.',
);
