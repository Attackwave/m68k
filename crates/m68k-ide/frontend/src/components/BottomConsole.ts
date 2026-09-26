//! Bottom Console & Error Output Panel.

import { AssembleErrorItem } from '../api';

export interface BottomConsoleProps {
  onErrorClick: (line: number) => void;
}

export function createBottomConsole(props: BottomConsoleProps): {
  element: HTMLElement;
  log: (msg: string, isError?: boolean) => void;
  setErrors: (errors: AssembleErrorItem[], warnings: string[]) => void;
  clear: () => void;
} {
  const container = document.createElement('div');
  container.className = 'h-44 bg-studio-sidebar border-t border-studio-border flex flex-col shrink-0 text-xs font-mono select-none';

  container.innerHTML = `
    <!-- Console Tabs -->
    <div class="h-8 border-b border-studio-border flex items-center justify-between px-3 bg-studio-bg shrink-0">
      <div class="flex items-center space-x-2">
        <button id="tab-build" class="px-2.5 py-1 text-[11px] font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5">
          <span>🔨 Build & Errors</span>
          <span id="error-badge" class="hidden px-1.5 py-0.2 bg-red-500/20 text-red-400 rounded-full text-[9px]">0</span>
        </button>
        <button id="tab-emu" class="px-2.5 py-1 text-[11px] font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5">
          <span>🎮 Emulator Logs</span>
        </button>
      </div>

      <button id="btn-clear-console" title="Clear Logs" class="p-1 text-studio-muted hover:text-white hover:bg-studio-panel rounded text-[10px]">
        🧹 Clear
      </button>
    </div>

    <!-- Panel 1: Build Output -->
    <div id="console-build-view" class="flex-1 overflow-y-auto p-2.5 space-y-1 bg-studio-bg text-studio-text">
      <div class="text-studio-muted">Ready. Press F7 or click Build to assemble project.</div>
    </div>

    <!-- Panel 2: Emulator Log View -->
    <div id="console-emu-view" class="flex-1 hidden overflow-y-auto p-2.5 space-y-1 bg-studio-bg text-studio-text">
      <div class="text-studio-muted">No emulator instance currently active.</div>
    </div>
  `;

  const buildView = container.querySelector('#console-build-view') as HTMLElement;
  const emuView = container.querySelector('#console-emu-view') as HTMLElement;
  const errorBadge = container.querySelector('#error-badge') as HTMLElement;

  const tabBuild = container.querySelector('#tab-build') as HTMLElement;
  const tabEmu = container.querySelector('#tab-emu') as HTMLElement;

  tabBuild.addEventListener('click', () => {
    tabBuild.className = 'px-2.5 py-1 text-[11px] font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5';
    tabEmu.className = 'px-2.5 py-1 text-[11px] font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5';
    buildView.classList.remove('hidden');
    emuView.classList.add('hidden');
  });

  tabEmu.addEventListener('click', () => {
    tabEmu.className = 'px-2.5 py-1 text-[11px] font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5';
    tabBuild.className = 'px-2.5 py-1 text-[11px] font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5';
    emuView.classList.remove('hidden');
    buildView.classList.add('hidden');
  });

  container.querySelector('#btn-clear-console')?.addEventListener('click', () => {
    buildView.innerHTML = '';
    emuView.innerHTML = '';
    errorBadge.classList.add('hidden');
  });

  function log(msg: string, isError: boolean = false) {
    const el = document.createElement('div');
    el.className = isError ? 'text-red-400 font-medium' : 'text-studio-text';
    el.textContent = `[${new Date().toLocaleTimeString()}] ${msg}`;
    buildView.appendChild(el);
    buildView.scrollTop = buildView.scrollHeight;
  }

  function setErrors(errors: AssembleErrorItem[], warnings: string[]) {
    buildView.innerHTML = '';

    if (errors.length === 0 && warnings.length === 0) {
      errorBadge.classList.add('hidden');
      log('Build succeeded! Output generated with 0 errors.', false);
      return;
    }

    errorBadge.textContent = `${errors.length}`;
    errorBadge.classList.remove('hidden');

    errors.forEach((err) => {
      const errEl = document.createElement('div');
      errEl.className = 'px-2 py-1 bg-red-500/10 border border-red-500/20 rounded cursor-pointer hover:bg-red-500/20 text-red-400 flex justify-between items-center transition';
      errEl.innerHTML = `
        <span class="flex items-center space-x-2">
          <span>❌</span>
          <span>${err.message}</span>
        </span>
        <span class="text-white bg-red-600 px-1.5 py-0.5 rounded text-[10px] font-bold">Line ${err.line}</span>
      `;
      errEl.addEventListener('click', () => props.onErrorClick(err.line));
      buildView.appendChild(errEl);
    });

    warnings.forEach((warn) => {
      const warnEl = document.createElement('div');
      warnEl.className = 'px-2 py-1 bg-amber-500/10 border border-amber-500/20 rounded text-amber-400';
      warnEl.textContent = `⚠️ ${warn}`;
      buildView.appendChild(warnEl);
    });
  }

  function clear() {
    buildView.innerHTML = '';
    emuView.innerHTML = '';
  }

  return { element: container, log, setErrors, clear };
}
