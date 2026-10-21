// Exercise the actual installed dependency code against the reported exploits.
const test = require('node:test');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const path = require('node:path');
const frontend = createRequire(path.resolve(__dirname, '../frontend/package.json'));
const editor = createRequire(frontend.resolve('@blocknote/core'));

test('Tiptap does not turn JSON __proto__ into executable inherited DOM attributes', () => {
  const { mergeAttributes } = editor('@tiptap/core');
  const hostile = JSON.parse('{"__proto__":{"src":"invalid://test","onerror":"attack()"}}');
  const attributes = mergeAttributes({ class: 'note' }, hostile);
  assert.equal(Object.getPrototypeOf(attributes), Object.prototype);
  const serialized = Object.create(null);
  for (const key in attributes) serialized[key] = attributes[key];
  assert.equal(serialized.onerror, undefined);
  assert.equal(serialized.src, undefined);
  assert.equal(attributes.class, 'note');
});
