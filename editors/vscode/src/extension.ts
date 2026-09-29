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

async function startClient(context: vscode.ExtensionContext) {
  const cpu = vscode.workspace.getConfiguration('m68k').get<string>('cpu', '68000');
  const serverOptions: ServerOptions = { command: tool(context, 'm68k-lsp'), args: ['--cpu', cpu] };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'm68k' }],
  };
  client = new LanguageClient('m68k', 'm68k Language Server', serverOptions, clientOptions);
  await client.start();
}

function buildTask(context: vscode.ExtensionContext, def: vscode.TaskDefinition, scope: vscode.WorkspaceFolder) {
  const cfg = vscode.workspace.getConfiguration('m68k', scope);
  const args = [def.file, '-f', def.format ?? 'hunk-exe', '-c', cfg.get<string>('cpu', '68000')];
  for (const dir of cfg.get<string[]>('includePaths', [])) args.push('-I', dir);
  if (def.output) args.push('-o', def.output);

  const task = new vscode.Task(
    def,
    scope,
    `build ${def.file}`,
    'm68k',
    new vscode.ProcessExecution(tool(context, 'm68k-asm'), args, { cwd: scope.uri.fsPath }),
    ['$m68k-asm', '$m68k-asm-diag']
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
      if (!config.program && doc?.languageId === 'm68k') {
        config.program = doc.uri.fsPath.replace(/\.[^./\\]+$/, '');
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

      const scope = doc && vscode.workspace.getWorkspaceFolder(doc.uri);
      if (config.build !== false && doc?.languageId === 'm68k' && scope) {
        await doc.save();
        const file = path.relative(scope.uri.fsPath, doc.uri.fsPath);
        const output = path.relative(scope.uri.fsPath, config.program);
        const code = await runTask(buildTask(context, { type: 'm68k', file, output }, scope));
        if (code !== 0) return undefined; // errors are in the Problems panel
      }
      return config;
    },
  };
}

export async function activate(context: vscode.ExtensionContext) {
  context.subscriptions.push(
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
      if (e.affectsConfiguration('m68k.cpu') || e.affectsConfiguration('m68k.toolsPath')) {
        vscode.commands.executeCommand('m68k.restartServer');
      }
    })
  );
  await startClient(context);
}

export function deactivate() {
  return client?.stop();
}
