const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('apps/web_app/web/app.js', 'utf8');
const handler = source.split('\n').find(line => line.startsWith('function focusWorkspaceRegion('));
assert.ok(handler, 'workspace focus handler exists');
const visits = [];
const context = vm.createContext({document: {getElementById(id) {
  return {
    scrollIntoView(options) { visits.push([id, 'scroll', options.behavior, options.block]); },
    focus(options) { visits.push([id, 'focus', options.preventScroll]); },
  };
}}});
vm.runInContext(handler, context);
context.focusWorkspaceRegion('workspace-pane');
context.focusWorkspaceRegion('jobs-pane');
context.focusWorkspaceRegion('settings-email');
assert.deepEqual(visits, [
  ['workspace-pane', 'scroll', 'instant', 'start'], ['workspace-pane', 'focus', true],
  ['jobs-pane', 'scroll', 'instant', 'start'], ['jobs-pane', 'focus', true],
]);
console.log('Workspace region navigation checks passed.');
