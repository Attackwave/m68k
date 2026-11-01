import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { LanguageClient, LanguageClientOptions, ServerOptions } from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

/// Resolves a tool binary: user setting, then the copy bundled in `bin/`, then PATH.
function tool(context: vscode.ExtensionContext, name: string): string {
  const exe = process.platform === 'win32' ? `${name}.exe` : name;
  const configured = vscode.workspace.getConfiguration('m68k').get<string>('toolsPath');
  for (const dir of [configured, context.asAbsolutePath('bin')]) {
    if (dir && fs.existsSync(path.join(dir, exe))) return path.join(dir, exe);
  }
  return exe;
}

/// `m68k.mainFile` as an absolute path, or undefined when not set.
function mainFile(): string | undefined {
  const name = vscode.workspace.getConfiguration('m68k').get<string>('mainFile');
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return name && root ? path.resolve(root, name) : undefined;
}

async function startClient(context: vscode.ExtensionContext) {
  const cfg = vscode.workspace.getConfiguration('m68k');
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? '';
  const args = ['--cpu', cfg.get<string>('cpu', '68000')];
  for (const dir of cfg.get<string[]>('includePaths', [])) args.push('-I', path.resolve(root, dir));
  const main = mainFile();
  if (main) args.push('--main', main);
  const serverOptions: ServerOptions = { command: tool(context, 'm68k-lsp'), args };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'm68k' }],
  };
  client = new LanguageClient('m68k', 'm68k Language Server', serverOptions, clientOptions);
  await client.start();
}

function buildTask(context: vscode.ExtensionContext, def: vscode.TaskDefinition, scope: vscode.WorkspaceFolder) {
  const cfg = vscode.workspace.getConfiguration('m68k', scope);
  const format = def.format ?? 'hunk-exe';
  const args = [def.file, '-f', format, '-c', cfg.get<string>('cpu', '68000')];
  // Line info embeds absolute source paths, so only debug builds carry it.
  if (def.linedebug && format === 'hunk-exe') args.push('--linedebug');
  for (const dir of cfg.get<string[]>('includePaths', [])) args.push('-I', dir);
  if (def.output) args.push('-o', def.output);

  const task = new vscode.Task(
    def,
    scope,
    `build ${def.file}`,
    'm68k',
    new vscode.ProcessExecution(tool(context, 'm68k-asm'), args, { cwd: scope.uri.fsPath }),
    ['$m68k-asm']
  );
  task.group = vscode.TaskGroup.Build;
  return task;
}

/// Runs a task and resolves with its exit code.
async function runTask(task: vscode.Task): Promise<number | undefined> {
  const execution = await vscode.tasks.executeTask(task);
  return new Promise((resolve) => {
    const sub = vscode.tasks.onDidEndTaskProcess((e) => {
      if (e.execution === execution) {
        sub.dispose();
        resolve(e.exitCode);
      }
    });
  });
}

/// F5 without launch.json: build the active file, debug its hunk executable.
function debugConfigurations(context: vscode.ExtensionContext): vscode.DebugConfigurationProvider {
  return {
    async resolveDebugConfiguration(_folder, config) {
      const doc = vscode.window.activeTextEditor?.document;
      if (!config.type && !config.request) {
        if (doc?.languageId !== 'm68k') {
          vscode.window.showErrorMessage('Open an m68k source file to debug it.');
          return undefined;
        }
        config = { type: 'm68k', request: 'launch', name: 'Debug in UAE', stopOnEntry: true };
      }
      // The program is the main file when one is set; the active file is
      // then usually one of its includes.
      const source = mainFile() ?? (doc?.languageId === 'm68k' ? doc.uri.fsPath : undefined);
      if (!config.program && source) {
        config.program = source.replace(/\.[^./\\]+$/, '');
      }
      const cfg = vscode.workspace.getConfiguration('m68k');
      config.emulator ??= cfg.get<string>('emulator') || undefined;
      config.emulatorArgs ??= cfg.get<string[]>('emulatorArgs', []);
      if (!config.emulator) {
        const pick = await vscode.window.showWarningMessage(
          'No emulator configured (m68k.emulator). Attach to one already running on port ' +
            (config.serverPort ?? 2345) +
            '?',
          'Attach',
          'Open Settings'
        );
        if (pick === 'Open Settings') vscode.commands.executeCommand('workbench.action.openSettings', 'm68k.emulator');
        if (pick !== 'Attach') return undefined;
      }

      const scope = source && vscode.workspace.getWorkspaceFolder(vscode.Uri.file(source));
      if (config.build !== false && source && scope) {
        await vscode.workspace.saveAll(false);
        const file = path.relative(scope.uri.fsPath, source);
        const output = path.relative(scope.uri.fsPath, config.program);
        const code = await runTask(buildTask(context, { type: 'm68k', file, output, linedebug: true }, scope));
        if (code !== 0) return undefined; // errors are in the Problems panel
      }
      return config;
    },
  };
}

interface CycleSum {
  min: number;
  max: number | null;
  instructions: number;
  unknown: number;
}

/// Status bar readout of the 68000 cycles in the selection.
function cycleReadout(): vscode.Disposable {
  const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  item.tooltip =
    '68000 cycles of the selected instructions (zero wait states). ' +
    'Chip RAM accesses can take longer while custom chip DMA is busy.';
  let pending: NodeJS.Timeout | undefined;

  const update = async (editor: vscode.TextEditor | undefined) => {
    const sel = editor?.selection;
    if (!client || !editor || !sel || sel.isEmpty || editor.document.languageId !== 'm68k') {
      item.hide();
      return;
    }
    // A selection ending at column 0 does not include that line.
    const endLine = sel.end.character === 0 && sel.end.line > sel.start.line ? sel.end.line - 1 : sel.end.line;
    const sum = await client.sendRequest<CycleSum | null>('m68k/cycleSum', {
      textDocument: { uri: editor.document.uri.toString() },
      startLine: sel.start.line,
      endLine,
    });
    if (!sum || sum.instructions === 0) {
      item.hide();
      return;
    }
    const cycles = sum.max === null ? `≥${sum.min}` : sum.min === sum.max ? `${sum.min}` : `${sum.min}–${sum.max}`;
    const unknown = sum.unknown ? ` (+${sum.unknown} unknown)` : '';
    item.text = `$(clock) ${cycles} cycles · ${sum.instructions} instr${unknown}`;
    item.show();
  };

  const schedule = (editor: vscode.TextEditor | undefined) => {
    clearTimeout(pending);
    pending = setTimeout(() => update(editor).catch(() => item.hide()), 100);
  };
  return vscode.Disposable.from(
    item,
    vscode.window.onDidChangeTextEditorSelection((e) => schedule(e.textEditor)),
    vscode.window.onDidChangeActiveTextEditor(schedule),
    { dispose: () => clearTimeout(pending) }
  );
}

export async function activate(context: vscode.ExtensionContext) {
  context.subscriptions.push(
    cycleReadout(),
    vscode.debug.registerDebugAdapterDescriptorFactory('m68k', {
      createDebugAdapterDescriptor: () => new vscode.DebugAdapterExecutable(tool(context, 'm68k-dap')),
    }),
    vscode.debug.registerDebugConfigurationProvider('m68k', debugConfigurations(context)),
    vscode.tasks.registerTaskProvider('m68k', {
      // One build task per open m68k document; tasks.json entries resolve through the same path.
      provideTasks: () =>
        vscode.workspace.textDocuments
          .filter((doc) => doc.languageId === 'm68k' && vscode.workspace.getWorkspaceFolder(doc.uri))
          .map((doc) => {
            const scope = vscode.workspace.getWorkspaceFolder(doc.uri)!;
            const file = path.relative(scope.uri.fsPath, doc.uri.fsPath);
            return buildTask(context, { type: 'm68k', file }, scope);
          }),
      resolveTask: (task) =>
        task.scope && typeof task.scope === 'object'
          ? buildTask(context, task.definition, task.scope)
          : undefined,
    }),
    vscode.commands.registerCommand('m68k.restartServer', async () => {
      await client?.stop();
      await startClient(context);
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (['m68k.cpu', 'm68k.toolsPath', 'm68k.includePaths', 'm68k.mainFile'].some((k) => e.affectsConfiguration(k))) {
        vscode.commands.executeCommand('m68k.restartServer');
      }
    })
  );
  await startClient(context);
}

export function deactivate() {
  return client?.stop();
}
