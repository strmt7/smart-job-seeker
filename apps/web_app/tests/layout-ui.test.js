const assert = require('node:assert/strict');
const { readSizes, splitSize, readDisplay } = require('../web/layout.js');
const saved = (value) => ({ getItem: () => value });

assert.deepEqual(readSizes(saved('{"jobs":0.7,"library":0.4,"review":0.6}')), {
  jobs: 0.7,
  library: 0.4,
  review: 0.6,
});
for (const raw of ['{', 'null', 'x'.repeat(257), '{"jobs":null,"review":"0.7","library":3}']) {
  assert.deepEqual(readSizes(saved(raw)), { jobs: 0.32, library: 0.44, review: 0.5 });
}
assert.deepEqual(
  readSizes({
    getItem() {
      throw Error('Browser storage denied');
    },
  }),
  { jobs: 0.32, library: 0.44, review: 0.5 },
);
assert.equal(splitSize(700, 0.5, 280, 420), null);
assert.equal(splitSize(NaN, 0.5, 280, 420), null);
assert.equal(splitSize(1000, Infinity, 280, 420), null);
for (const width of [710, 850, 1024, 1440, 2560, 3840]) {
  for (const ratio of [-2, 0, 0.2, 0.5, 0.9, 2]) {
    const first = splitSize(width, ratio, 280, 420);
    assert.ok(first >= 280 && first >= (width - 10) * 0.15);
    assert.ok(width - 10 - first >= 420);
    assert.ok(first <= (width - 10) * 0.85);
  }
}
console.log('Pane sizes preserve both minimums, maximum proportions and safe tab preferences.');

assert.deepEqual(readDisplay(saved('{"density":"compact","motion":"reduced"}')), {
  density: 'compact',
  motion: 'reduced',
});
for (const raw of ['null', '{', '{"density":"invalid","motion":true}', 'x'.repeat(129)])
  assert.deepEqual(readDisplay(saved(raw)), { density: 'standard', motion: 'system' });
