//! VS Code Style Bottom Panel Component for m68k Studio.

import { IdeDiagnosticItem } from '../api';

export type BottomTabId = 'problems' | 'output' | 'disasm' | 'hex' | 'hwregs';

export interface BottomPanelProps {
  initialTab?: BottomTabId;
  onSelectProblem?: (line: number, col: number) => void;
  onClosePanel?: () => void;
}

export function createBottomPanel(props: BottomPanelProps): {
  element: HTMLElement;
  setTab: (tabId: BottomTabId) => void;
  setProblems: (errors: Array<{ line: number; message: string } | string>, warnings: string[], diags?: IdeDiagnosticItem[]) => void;
  log: (text: string, isError?: boolean) => void;
  clearLog: () => void;
  attachCustomView: (tabId: BottomTabId, viewElement: HTMLElement) => void;
} {
  const container = document.createElement('div');
  container.className =
    'h-56 bg-studio-sidebar border-t border-studio-border flex flex-col shrink-0 text-xs font-sans select-none relative z-10';

  let activeTab: BottomTabId = props.initialTab || 'problems';
  let isMaximized = false;

  container.innerHTML = `
    <!-- Top Header Bar with Tabs -->
    <div class="h-8 border-b border-studio-border bg-studio-bg flex items-center justify-between px-2 shrink-0">
      <div class="flex items-center space-x-1 h-full" id="bpanel-tabs-group">
        <button class="bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 transition cursor-pointer border-b-2" data-tab="problems" aria-label="Tab: Probleme">
          <span>PROBLEME</span>
          <span id="bpanel-badge-problems" class="px-1.5 py-0.2 rounded-full text-[9px] font-mono bg-studio-panel border border-studio-border text-studio-muted">0</span>
        </button>
        <button class="bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 transition cursor-pointer border-b-2" data-tab="output" aria-label="Tab: Ausgabe Build">
          <span>AUSGABE (BUILD)</span>
        </button>
        <button class="bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 transition cursor-pointer border-b-2" data-tab="disasm" aria-label="Tab: Disassembly">
          <span>DISASSEMBLY</span>
        </button>
        <button class="bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 transition cursor-pointer border-b-2" data-tab="hex" aria-label="Tab: Hex Dump">
          <span>HEX-DUMP</span>
        </button>
        <button class="bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 transition cursor-pointer border-b-2" data-tab="hwregs" aria-label="Tab: Hardware Register">
          <span>HARDWARE-REGISTER</span>
        </button>
      </div>

      <!-- Right Panel Controls -->
      <div class="flex items-center space-x-1 text-studio-muted">
        <button id="bpanel-btn-clear" aria-label="Ausgabe leeren" title="Ausgabe leeren" class="p-1 hover:text-white hover:bg-studio-hover rounded transition cursor-pointer text-xs">
          🗑️
        </button>
        <button id="bpanel-btn-maximize" aria-label="Panel maximieren oder verkleinern" title="Panel maximieren/verkleinern" class="p-1 hover:text-white hover:bg-studio-hover rounded transition cursor-pointer text-xs">
          ⤢
        </button>
        <button id="bpanel-btn-close" aria-label="Panel schließen (Ctrl+J)" title="Panel schließen (Ctrl+J)" class="p-1 hover:text-white hover:bg-studio-hover rounded transition cursor-pointer text-xs">
          ✕
        </button>
      </div>
    </div>

    <!-- Panel Content Area -->
    <div class="flex-1 overflow-hidden relative" id="bpanel-content-area">
      <!-- VIEW 1: Problems -->
      <div id="bview-problems" class="absolute inset-0 overflow-y-auto p-2 space-y-1 font-mono text-xs">
        <div class="text-center py-6 text-studio-muted font-sans text-xs">Keine Probleme im Workspace erkannt.</div>
      </div>

      <!-- VIEW 2: Output -->
      <div id="bview-output" class="absolute inset-0 overflow-y-auto p-2 space-y-0.5 font-mono text-[11px] bg-studio-bg hidden">
        <div class="text-studio-muted">m68k Studio Build System bereit. Drücke F7 zum Kompilieren.</div>
      </div>

      <!-- Custom Views (Disasm, Hex, Hardware Registers) are mounted dynamically -->
      <div id="bview-disasm" class="absolute inset-0 overflow-hidden hidden"></div>
      <div id="bview-hex" class="absolute inset-0 overflow-hidden hidden"></div>
      <div id="bview-hwregs" class="absolute inset-0 overflow-hidden hidden"></div>
    </div>
  `;

  const tabsGroup = container.querySelector('#bpanel-tabs-group') as HTMLElement;
  const viewProblems = container.querySelector('#bview-problems') as HTMLElement;
  const viewOutput = container.querySelector('#bview-output') as HTMLElement;
  const viewDisasm = container.querySelector('#bview-disasm') as HTMLElement;
  const viewHex = container.querySelector('#bview-hex') as HTMLElement;
  const viewHwregs = container.querySelector('#bview-hwregs') as HTMLElement;

  const viewsMap: Record<BottomTabId, HTMLElement> = {
    problems: viewProblems,
    output: viewOutput,
    disasm: viewDisasm,
    hex: viewHex,
    hwregs: viewHwregs,
  };

  function updateTabsUI() {
    tabsGroup.querySelectorAll('.bpanel-tab').forEach((tab) => {
      const tid = tab.getAttribute('data-tab') as BottomTabId;
      const isSelected = tid === activeTab;

      if (isSelected) {
        tab.className =
          'bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 text-white border-b-2 border-b-blue-500 bg-studio-panel/40';
      } else {
        tab.className =
          'bpanel-tab h-full px-2.5 font-bold uppercase tracking-wider text-[11px] flex items-center space-x-1.5 text-studio-muted hover:text-studio-text border-b-2 border-b-transparent';
      }
    });

    Object.entries(viewsMap).forEach(([tid, el]) => {
      if (tid === activeTab) {
        el.classList.remove('hidden');
      } else {
        el.classList.add('hidden');
      }
    });
  }

  tabsGroup.querySelectorAll('.bpanel-tab').forEach((tab) => {
    tab.addEventListener('click', () => {
      activeTab = tab.getAttribute('data-tab') as BottomTabId;
      updateTabsUI();
    });
  });

  // Maximize toggle
  container.querySelector('#bpanel-btn-maximize')?.addEventListener('click', () => {
    isMaximized = !isMaximized;
    if (isMaximized) {
      container.classList.remove('h-56');
      container.classList.add('h-[480px]');
    } else {
      container.classList.remove('h-[480px]');
      container.classList.add('h-56');
    }
  });

  // Close panel
  container.querySelector('#bpanel-btn-close')?.addEventListener('click', () => {
    if (props.onClosePanel) props.onClosePanel();
  });

  // Clear output log
  container.querySelector('#bpanel-btn-clear')?.addEventListener('click', () => {
    viewOutput.innerHTML = '';
  });

  updateTabsUI();

  function setTab(tabId: BottomTabId) {
    activeTab = tabId;
    updateTabsUI();
  }

  function setProblems(errors: Array<{ line: number; message: string } | string>, warnings: string[], diags?: IdeDiagnosticItem[]) {
    const badge = container.querySelector('#bpanel-badge-problems');
    const totalCount = errors.length + warnings.length + (diags ? diags.length : 0);

    if (badge) {
      badge.textContent = `${totalCount}`;
      if (errors.length > 0 || (diags && diags.some((d) => d.severity === 1))) {
        badge.className = 'px-1.5 py-0.2 rounded-full text-[9px] font-mono bg-red-500/20 text-red-400 font-bold';
      } else if (totalCount > 0) {
        badge.className = 'px-1.5 py-0.2 rounded-full text-[9px] font-mono bg-amber-500/20 text-amber-300 font-bold';
      } else {
        badge.className = 'px-1.5 py-0.2 rounded-full text-[9px] font-mono bg-studio-panel border border-studio-border text-studio-muted';
      }
    }

    if (totalCount === 0) {
      viewProblems.innerHTML = '<div class="text-center py-6 text-studio-muted font-sans text-xs">Keine Probleme im Workspace erkannt.</div>';
      return;
    }

    let html = '';

    // Add Build Errors
    for (const err of errors) {
      const line = typeof err === 'string' ? (err.match(/:(\d+):/) ? parseInt(err.match(/:(\d+):/)![1], 10) : 1) : err.line;
      const msg = typeof err === 'string' ? err : err.message;
      html += `
        <div class="problem-item px-2.5 py-1.5 bg-red-500/10 border border-red-500/20 rounded-lg flex items-center justify-between text-red-300 hover:bg-red-500/20 transition cursor-pointer" data-line="${line}">
          <div class="flex items-center space-x-2 truncate">
            <span class="text-xs shrink-0">❌</span>
            <span class="truncate">${msg}</span>
          </div>
          <span class="text-[10px] font-mono shrink-0 ml-2 opacity-80">Zeile ${line}</span>
        </div>
      `;
    }

    // Add Build Warnings
    for (const warn of warnings) {
      const match = warn.match(/:(\d+):/);
      const line = match ? parseInt(match[1], 10) : 1;
      html += `
        <div class="problem-item px-2.5 py-1.5 bg-amber-500/10 border border-amber-500/20 rounded-lg flex items-center justify-between text-amber-300 hover:bg-amber-500/20 transition cursor-pointer" data-line="${line}">
          <div class="flex items-center space-x-2 truncate">
            <span class="text-xs shrink-0">⚠️</span>
            <span class="truncate">${warn}</span>
          </div>
          <span class="text-[10px] font-mono shrink-0 ml-2 opacity-80">Zeile ${line}</span>
        </div>
      `;
    }

    // Add LSP Diagnostics
    if (diags) {
      for (const d of diags) {
        const isErr = d.severity === 1;
        const line = d.start_line + 1;
        const col = d.start_col + 1;
        html += `
          <div class="problem-item px-2.5 py-1.5 ${
            isErr ? 'bg-red-500/10 border-red-500/20 text-red-300' : 'bg-amber-500/10 border-amber-500/20 text-amber-300'
          } border rounded-lg flex items-center justify-between hover:opacity-90 transition cursor-pointer" data-line="${line}" data-col="${col}">
            <div class="flex items-center space-x-2 truncate">
              <span class="text-xs shrink-0">${isErr ? '❌' : '⚠️'}</span>
              <span class="truncate">${d.message}</span>
            </div>
            <span class="text-[10px] font-mono shrink-0 ml-2 opacity-80">Ln ${line}, Col ${col}</span>
          </div>
        `;
      }
    }

    viewProblems.innerHTML = html;

    viewProblems.querySelectorAll('.problem-item').forEach((item) => {
      item.addEventListener('click', () => {
        const line = parseInt(item.getAttribute('data-line') || '1', 10);
        const col = parseInt(item.getAttribute('data-col') || '1', 10);
        if (props.onSelectProblem) props.onSelectProblem(line, col);
      });
    });
  }

  function log(text: string, isError: boolean = false) {
    const time = new Date().toLocaleTimeString();
    const row = document.createElement('div');
    row.className = isError ? 'text-red-400 font-semibold' : 'text-studio-text';
    row.textContent = `[${time}] ${text}`;
    viewOutput.appendChild(row);
    viewOutput.scrollTop = viewOutput.scrollHeight;
  }

  function clearLog() {
    viewOutput.innerHTML = '';
  }

  function attachCustomView(tabId: BottomTabId, viewElement: HTMLElement) {
    const containerEl = viewsMap[tabId];
    if (containerEl) {
      containerEl.innerHTML = '';
      containerEl.appendChild(viewElement);
    }
  }

  return {
    element: container,
    setTab,
    setProblems,
    log,
    clearLog,
    attachCustomView,
  };
}
