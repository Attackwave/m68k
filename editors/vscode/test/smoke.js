// Smoke test: talks JSON-RPC to the real m68k-lsp and checks the problem
// matcher regex against real m68k-asm output. Run: node test/smoke.js
const { spawnSync, spawn } = require('child_process');
const assert = require('assert');
const fs = require('fs');
const os = require('os');
const path = require('path');

const bin = (name) => path.join(__dirname, '..', 'bin', process.platform === 'win32' ? `${name}.exe` : name);
const pkg = require('../package.json');

// Problem matchers: must pick file/line out of real assembler errors, including
// ones raised inside a macro expansion.
const [fatal, diag] = pkg.contributes.problemMatchers.map((pm) => pm.pattern);
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'm68k-'));
for (const [src, wantLine] of [
  ['start:\n  bogus d0\n', '2'],
  ['m macro\n  bogus d0\n  endm\n  m\n', '4'],
]) {
  fs.writeFileSync(path.join(dir, 'bad.s'), src);
  const asm = spawnSync(bin('m68k-asm'), ['bad.s', '-f', 'hunk-exe'], { cwd: dir, encoding: 'utf8' });
  const line = (asm.stderr + asm.stdout).trim().split('\n')[0];
  const m = new RegExp(fatal.regexp).exec(line);
  assert(m, `problem matcher did not match: ${line}`);
  assert.deepStrictEqual([m[fatal.file], m[fatal.line]], ['bad.s', wantLine]);
}
// The non-fatal diagnostic format, as written by format_diagnostic in m68k-asm.rs.
const d = new RegExp(diag.regexp).exec('src/x.s:7 (in macro m): warning: odd address');
assert.deepStrictEqual([d[diag.file], d[diag.line], d[diag.severity]], ['src/x.s', '7', 'warning']);

// LSP: initialize, open a broken document, expect a diagnostic on line 1 (0-based).
const lsp = spawn(bin('m68k-lsp'), ['--cpu', '68000']);
const send = (msg) => {
  const body = JSON.stringify({ jsonrpc: '2.0', ...msg });
  lsp.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
};
let buf = '';
const timer = setTimeout(() => { lsp.kill(); assert.fail('no diagnostics from m68k-lsp'); }, 5000);
lsp.stdout.on('data', (chunk) => {
  buf += chunk;
  for (;;) {
    const hdr = /Content-Length: (\d+)\r\n\r\n/.exec(buf);
    if (!hdr || buf.length < hdr.index + hdr[0].length + +hdr[1]) return;
    const msg = JSON.parse(buf.substr(hdr.index + hdr[0].length, +hdr[1]));
    buf = buf.slice(hdr.index + hdr[0].length + +hdr[1]);
    if (msg.id === 1) {
      send({ method: 'initialized', params: {} });
      send({
        method: 'textDocument/didOpen',
        params: { textDocument: { uri: 'file:///tmp/t.s', languageId: 'm68k', version: 1, text: 'start:\n  bogus d0\n' } },
      });
    }
    if (msg.method === 'textDocument/publishDiagnostics' && msg.params.diagnostics.length) {
      assert.strictEqual(msg.params.diagnostics[0].range.start.line, 1);
      clearTimeout(timer);
      lsp.kill();
      console.log('ok');
    }
  }
});
send({ id: 1, method: 'initialize', params: { processId: null, rootUri: null, capabilities: {} } });
