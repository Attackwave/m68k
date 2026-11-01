// Smoke test: talks JSON-RPC to the real m68k-lsp and checks the problem
// matcher regex against real m68k-asm output. Run: node test/smoke.js
const { spawnSync, spawn } = require('child_process');
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');

const bin = (name) => path.join(__dirname, '..', 'bin', process.platform === 'win32' ? `${name}.exe` : name);
const pkg = require('../package.json');

// Problem matcher: must pick file, line and severity out of real assembler
// output, including an error raised inside a macro expansion. Every
// diagnostic line has the same shape; the summary line after them does
// not name a file:line and must not match.
const [diag] = pkg.contributes.problemMatchers.map((pm) => pm.pattern);
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'm68k-'));
for (const [src, wantLine] of [
  ['start:\n  bogus d0\n', '2'],
  ['m macro\n  bogus d0\n  endm\n  m\n', '4'],
]) {
  fs.writeFileSync(path.join(dir, 'bad.s'), src);
  const asm = spawnSync(bin('m68k-asm'), ['bad.s', '-f', 'hunk-exe'], { cwd: dir, encoding: 'utf8' });
  const lines = (asm.stderr + asm.stdout).trim().split('\n');
  const m = new RegExp(diag.regexp).exec(lines[0]);
  assert(m, `problem matcher did not match: ${lines[0]}`);
  assert.deepStrictEqual([m[diag.file], m[diag.line], m[diag.severity]], ['bad.s', wantLine, 'error']);
  assert(!new RegExp(diag.regexp).exec(lines[lines.length - 1]), `summary line matched: ${lines[lines.length - 1]}`);
}
const d = new RegExp(diag.regexp).exec('src/x.s:7 (in macro m): warning: odd address');
assert.deepStrictEqual([d[diag.file], d[diag.line], d[diag.severity]], ['src/x.s', '7', 'warning']);

// LSP: initialize, open a document with an error, expect a diagnostic on
// line 1 (0-based), cycle hints and a cycle sum for the valid lines.
const lsp = spawn(bin('m68k-lsp'), ['--cpu', '68000']);
const send = (msg) => {
  const body = JSON.stringify({ jsonrpc: '2.0', ...msg });
  lsp.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
};
const TEXT = 'start:\n  bogus d0\n  moveq #0,d0\n  rts\n  move.w #$8380,DMACON(a6)\n';
const pending = new Set(['diagnostics', 'hints', 'sum', 'hover']);
// Content-Length counts bytes, so frames are cut from a Buffer, not a string.
let buf = Buffer.alloc(0);
const timer = setTimeout(() => { lsp.kill(); assert.fail(`m68k-lsp did not answer: ${[...pending]}`); }, 5000);
lsp.stdout.on('data', (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  for (;;) {
    const sep = buf.indexOf('\r\n\r\n');
    if (sep < 0) return;
    const len = +/Content-Length: (\d+)/.exec(buf.subarray(0, sep).toString())[1];
    if (buf.length < sep + 4 + len) return;
    const msg = JSON.parse(buf.subarray(sep + 4, sep + 4 + len).toString('utf8'));
    buf = buf.subarray(sep + 4 + len);
    if (msg.id === 1) {
      send({ method: 'initialized', params: {} });
      send({
        method: 'textDocument/didOpen',
        params: { textDocument: { uri: 'file:///tmp/t.s', languageId: 'm68k', version: 1, text: TEXT } },
      });
      const doc = { uri: 'file:///tmp/t.s' };
      const range = { start: { line: 0, character: 0 }, end: { line: 3, character: 0 } };
      send({ id: 2, method: 'textDocument/inlayHint', params: { textDocument: doc, range } });
      send({ id: 3, method: 'm68k/cycleSum', params: { textDocument: doc, startLine: 2, endLine: 3 } });
      send({ id: 4, method: 'textDocument/hover', params: { textDocument: doc, position: { line: 4, character: 18 } } });
    }
    if (msg.id === 2) {
      assert.deepStrictEqual(msg.result.map((h) => [h.position.line, h.label]), [[2, '(4 cyc)'], [3, '(16 cyc)']]);
      pending.delete('hints');
    }
    if (msg.id === 3) {
      assert.deepStrictEqual(msg.result, { min: 20, max: 20, instructions: 2, unknown: 0 });
      pending.delete('sum');
    }
    if (msg.id === 4) {
      assert.match(msg.result.contents.value, /SET DMAEN\|BPLEN\|COPEN/);
      pending.delete('hover');
    }
    if (msg.method === 'textDocument/publishDiagnostics' && msg.params.diagnostics.length) {
      assert.strictEqual(msg.params.diagnostics[0].range.start.line, 1);
      pending.delete('diagnostics');
    }
    if (pending.size === 0) {
      clearTimeout(timer);
      lsp.kill();
      console.log('ok');
      return;
    }
  }
});
send({ id: 1, method: 'initialize', params: { processId: null, rootUri: null, capabilities: {} } });

// Main-file mode: an included file opened on its own is checked as part of
// the program, so a symbol from the includer is known and only the file's
// own fault is reported, on its own line, from the unsaved text. Once the
// fault is edited away, the flag check runs over the whole program and
// warns about the TST whose flags the MOVEQ overwrites.
fs.writeFileSync(path.join(dir, 'main.s'), 'SIZE equ 4\n  include "inc.s"\n  rts\n');
fs.writeFileSync(path.join(dir, 'inc.s'), '  moveq #SIZE,d0\n');
const unit = spawn(bin('m68k-lsp'), ['--main', path.join(dir, 'main.s'), '--flag-check']);
const sendUnit = (msg) => {
  const body = JSON.stringify({ jsonrpc: '2.0', ...msg });
  unit.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
};
// A file URI is not 'file://' + path: a Windows path has a drive letter and backslashes.
const incUri = require('url').pathToFileURL(path.join(dir, 'inc.s')).href;
let unitBuf = Buffer.alloc(0);
const unitTimer = setTimeout(() => { unit.kill(); assert.fail('m68k-lsp --main did not publish diagnostics'); }, 5000);
let unitStep = 0;
unit.stdout.on('data', (chunk) => {
  unitBuf = Buffer.concat([unitBuf, chunk]);
  for (;;) {
    const sep = unitBuf.indexOf('\r\n\r\n');
    if (sep < 0) return;
    const len = +/Content-Length: (\d+)/.exec(unitBuf.subarray(0, sep).toString())[1];
    if (unitBuf.length < sep + 4 + len) return;
    const msg = JSON.parse(unitBuf.subarray(sep + 4, sep + 4 + len).toString('utf8'));
    unitBuf = unitBuf.subarray(sep + 4 + len);
    if (msg.id === 1) {
      sendUnit({ method: 'initialized', params: {} });
      sendUnit({
        method: 'textDocument/didOpen',
        params: { textDocument: { uri: incUri, languageId: 'm68k', version: 1, text: '  moveq #SIZE,d0\n  bogus d0\n' } },
      });
    }
    if (msg.method === 'textDocument/publishDiagnostics' && msg.params.uri === incUri && unitStep === 0) {
      assert.deepStrictEqual(msg.params.diagnostics.map((d) => d.range.start.line), [1]);
      assert.match(msg.params.diagnostics[0].message, /BOGUS/);
      unitStep = 1;
      sendUnit({
        method: 'textDocument/didChange',
        params: {
          textDocument: { uri: incUri, version: 2 },
          contentChanges: [{ text: '  tst.w d0\n  moveq #SIZE,d0\n' }],
        },
      });
    } else if (msg.method === 'textDocument/publishDiagnostics' && msg.params.uri === incUri && unitStep === 1) {
      assert.deepStrictEqual(msg.params.diagnostics.map((d) => [d.range.start.line, d.severity]), [[0, 2]]);
      assert.match(msg.params.diagnostics[0].message, /TST/);
      clearTimeout(unitTimer);
      unit.kill();
      console.log('ok (main file, flag check)');
      return;
    }
  }
});
sendUnit({ id: 1, method: 'initialize', params: { processId: null, rootUri: null, capabilities: {} } });
