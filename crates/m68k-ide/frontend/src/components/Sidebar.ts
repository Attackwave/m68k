//! Left Sidebar Component with Explorer, ADF Floppy Manager, and Symbol Outline.

import { AdfInfoResponse } from '../api';

export interface SidebarProps {
  files: Array<{ name: string; path: string; isDir?: boolean }>;
  activeFile: string;
  onFileSelect: (path: string) => void;
  onNewFile: () => void;
  onInspectAdf: (bytes: number[]) => void;
  adfInfo: AdfInfoResponse | null;
}

export function createSidebar(props: SidebarProps): { element: HTMLElement; updateAdf: (info: AdfInfoResponse) => void; updateSymbols: (symbols: any[]) => void } {
  const sidebar = document.createElement('aside');
  sidebar.className = 'w-64 bg-studio-sidebar border-r border-studio-border flex flex-col shrink-0 text-xs select-none';

  sidebar.innerHTML = `
    <!-- Sidebar Tabs -->
    <div class="h-9 border-b border-studio-border flex items-center px-2 space-x-1 bg-studio-bg shrink-0">
      <button id="tab-files" class="px-2.5 py-1 text-xs font-medium rounded text-white bg-studio-panel border border-studio-border flex items-center space-x-1.5">
        <span>📁 Files</span>
      </button>
      <button id="tab-floppy" class="px-2.5 py-1 text-xs font-medium rounded text-studio-muted hover:text-white hover:bg-studio-panel transition flex items-center space-x-1.5">
        <span>💾 Floppy (ADF)</span>
      </button>
      <button id="tab-symbols" class="px-2.5 py-1 text-xs font-medium rounded text-studio-muted hover:text-white hover:bg-studio-panel transition flex items-center space-x-1.5">
        <span>🌳 Outline</span>
      </button>
    </div>

    <!-- Panel 1: File Explorer -->
    <div id="view-files" class="flex-1 flex flex-col overflow-y-auto p-2">
      <div class="flex items-center justify-between px-1 mb-1.5 text-studio-muted uppercase font-bold text-[10px] tracking-wider">
        <span>Project Explorer</span>
        <button id="btn-add-file" title="New Source File" class="p-1 hover:text-white hover:bg-studio-panel rounded">
          ➕
        </button>
      </div>
      <div id="file-list" class="space-y-0.5">
        ${props.files
          .map(
            (f) => `
          <div class="file-item px-2 py-1.5 rounded cursor-pointer flex items-center space-x-2 transition ${
            f.path === props.activeFile ? 'bg-blue-600/20 text-blue-400 font-medium' : 'text-studio-text hover:bg-studio-hover'
          }" data-path="${f.path}">
            <span>${f.isDir ? '📁' : '📄'}</span>
            <span class="truncate">${f.name}</span>
          </div>
        `
          )
          .join('')}
      </div>
    </div>

    <!-- Panel 2: Floppy ADF Manager -->
    <div id="view-floppy" class="flex-1 hidden flex-col overflow-y-auto p-2 space-y-3">
      <div class="flex items-center justify-between px-1 text-studio-muted uppercase font-bold text-[10px] tracking-wider">
        <span>Amiga Floppy (ADF)</span>
      </div>

      <div class="p-2.5 bg-studio-panel border border-studio-border rounded text-xs space-y-1.5">
        <div class="flex justify-between">
          <span class="text-studio-muted">Disk:</span>
          <span id="adf-name" class="font-semibold text-white">AmigaDisk</span>
        </div>
        <div class="flex justify-between">
          <span class="text-studio-muted">Filesystem:</span>
          <span id="adf-fs" class="text-emerald-400 font-mono">OFS / AmigaDOS</span>
        </div>
        <div class="flex justify-between">
          <span class="text-studio-muted">Free Space:</span>
          <span id="adf-free" class="text-white font-mono">880 KB / 1760 Blk</span>
        </div>
      </div>

      <!-- ADF Files List -->
      <div class="space-y-1">
        <div class="text-[11px] font-semibold text-studio-muted px-1">Contents in Disk Root:</div>
        <div id="adf-file-list" class="space-y-0.5 max-h-48 overflow-y-auto bg-studio-bg p-1 rounded border border-studio-border">
          <div class="text-center py-4 text-studio-muted text-xs">Assemble to generate ADF contents</div>
        </div>
      </div>
    </div>

    <!-- Panel 3: Symbols Outline -->
    <div id="view-symbols" class="flex-1 hidden flex-col overflow-y-auto p-2">
      <div class="flex items-center justify-between px-1 mb-1.5 text-studio-muted uppercase font-bold text-[10px] tracking-wider">
        <span>Symbols & Outline</span>
      </div>
      <div id="symbols-list" class="space-y-0.5">
        <div class="text-center py-4 text-studio-muted text-xs">No symbols indexed</div>
      </div>
    </div>
  `;

  // Tab switching logic
  const tabFiles = sidebar.querySelector('#tab-files') as HTMLElement;
  const tabFloppy = sidebar.querySelector('#tab-floppy') as HTMLElement;
  const tabSymbols = sidebar.querySelector('#tab-symbols') as HTMLElement;

  const viewFiles = sidebar.querySelector('#view-files') as HTMLElement;
  const viewFloppy = sidebar.querySelector('#view-floppy') as HTMLElement;
  const viewSymbols = sidebar.querySelector('#view-symbols') as HTMLElement;

  function switchTab(activeTab: HTMLElement, activeView: HTMLElement) {
    [tabFiles, tabFloppy, tabSymbols].forEach((t) => {
      t.className = 'px-2.5 py-1 text-xs font-medium rounded text-studio-muted hover:text-white hover:bg-studio-panel transition flex items-center space-x-1.5';
    });
    [viewFiles, viewFloppy, viewSymbols].forEach((v) => {
      v.classList.add('hidden');
      v.classList.remove('flex');
    });

    activeTab.className = 'px-2.5 py-1 text-xs font-medium rounded text-white bg-studio-panel border border-studio-border flex items-center space-x-1.5';
    activeView.classList.remove('hidden');
    activeView.classList.add('flex');
  }

  tabFiles.addEventListener('click', () => switchTab(tabFiles, viewFiles));
  tabFloppy.addEventListener('click', () => switchTab(tabFloppy, viewFloppy));
  tabSymbols.addEventListener('click', () => switchTab(tabSymbols, viewSymbols));

  sidebar.querySelector('#btn-add-file')?.addEventListener('click', props.onNewFile);

  sidebar.querySelectorAll('.file-item').forEach((item) => {
    item.addEventListener('click', (e) => {
      const path = (e.currentTarget as HTMLElement).getAttribute('data-path');
      if (path) props.onFileSelect(path);
    });
  });

  function updateAdf(info: AdfInfoResponse) {
    const nameEl = sidebar.querySelector('#adf-name');
    const fsEl = sidebar.querySelector('#adf-fs');
    const freeEl = sidebar.querySelector('#adf-free');
    const listEl = sidebar.querySelector('#adf-file-list');

    if (nameEl) nameEl.textContent = info.volume_name;
    if (fsEl) fsEl.textContent = info.is_ffs ? 'FFS (Fast File System)' : 'OFS (Original File System)';
    if (freeEl) freeEl.textContent = `${info.free_blocks * 512 / 1024} KB (${info.free_blocks} Blocks)`;

    if (listEl) {
      if (info.entries.length === 0) {
        listEl.innerHTML = '<div class="text-center py-2 text-studio-muted text-xs">Disk is empty</div>';
      } else {
        listEl.innerHTML = info.entries
          .map(
            (e) => `
            <div class="px-2 py-1 rounded flex items-center justify-between text-studio-text hover:bg-studio-hover">
              <span class="flex items-center space-x-1.5">
                <span>${e.is_dir ? '📁' : '💾'}</span>
                <span>${e.name}</span>
              </span>
              <span class="text-studio-muted text-[10px] font-mono">${e.size} B</span>
            </div>
          `
          )
          .join('');
      }
    }
  }

  function updateSymbols(symbols: any[]) {
    const listEl = sidebar.querySelector('#symbols-list');
    if (!listEl) return;

    if (symbols.length === 0) {
      listEl.innerHTML = '<div class="text-center py-4 text-studio-muted text-xs">No symbols indexed</div>';
      return;
    }

    listEl.innerHTML = symbols
      .map(
        (s) => `
        <div class="px-2 py-1 rounded cursor-pointer flex items-center justify-between hover:bg-studio-hover text-studio-text">
          <span class="font-mono text-xs text-blue-400 font-semibold">${s.name}</span>
          <span class="text-studio-muted text-[10px]">L${s.line + 1}</span>
        </div>
      `
      )
      .join('');
  }

  return { element: sidebar, updateAdf, updateSymbols };
}
