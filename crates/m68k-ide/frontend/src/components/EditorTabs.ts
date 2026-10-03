//! VS Code Multi-File Tab Bar & Breadcrumbs Component for m68k Studio.

export interface EditorTabsProps {
  openFiles: string[];
  activeFile: string;
  dirtyFiles: Set<string>;
  isAsmPreviewActive?: boolean;
  onSelectTab: (path: string) => void;
  onCloseTab: (path: string) => void;
  onNewFile: () => void;
  onToggleAsmPreview?: () => void;
}

export function createEditorTabs(props: EditorTabsProps): {
  element: HTMLElement;
  updateTabs: (
    openFiles: string[],
    activeFile: string,
    dirtyFiles: Set<string>,
    isAsmPreviewActive?: boolean
  ) => void;
} {
  const container = document.createElement('div');
  container.className =
    'flex flex-col bg-studio-sidebar border-b border-studio-border shrink-0 select-none text-xs font-sans';

  let currentOpenFiles = [...props.openFiles];
  let currentActiveFile = props.activeFile;
  let currentDirtyFiles = new Set(props.dirtyFiles);
  let isAsmPreview = !!props.isAsmPreviewActive;

  container.innerHTML = `
    <!-- Top Tab Strip -->
    <div class="h-8 flex items-center justify-between overflow-x-auto bg-studio-bg shrink-0" id="editor-tabs-strip">
      <div class="flex items-center space-x-0.5 overflow-x-auto h-full flex-1" id="tabs-list-container">
        <!-- Rendered tabs -->
      </div>
      <!-- Right Editor Actions -->
      <div class="flex items-center space-x-1.5 px-2 shrink-0 bg-studio-bg border-l border-studio-border/40" id="editor-right-actions">
        <!-- Optional ASM Preview Toggle -->
        <button id="tabs-btn-asm-preview" title="Live M68K-Assembler-Vorschau umschalten" class="px-2 py-0.5 rounded text-[10px] font-mono flex items-center space-x-1 border transition cursor-pointer hidden">
          <span>⚡ ASM-Vorschau</span>
        </button>
        <button id="tabs-btn-new" aria-label="Neue Datei erstellen" title="Neue Datei erstellen" class="p-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white transition cursor-pointer">
          ➕
        </button>
      </div>
    </div>

    <!-- Breadcrumbs Bar -->
    <div class="h-6 px-4 bg-studio-sidebar/70 flex items-center justify-between text-[11px] text-studio-muted border-t border-studio-border/30 shrink-0 font-mono" id="editor-breadcrumbs-bar">
      <div class="flex items-center space-x-1 truncate" id="editor-breadcrumbs">
        <!-- Dynamically rendered breadcrumbs -->
      </div>
      <div class="text-[10px] text-studio-muted shrink-0" id="editor-mode-hint"></div>
    </div>
  `;

  function getFileMeta(path: string): { icon: string; name: string } {
    if (path === 'tool:blitter') return { icon: '🧮', name: 'Blitter Studio' };
    if (path === 'tool:bitplanes') return { icon: '🎨', name: 'Bitplane Studio' };
    if (path === 'tool:copper') return { icon: '🌈', name: 'Copper Visualizer' };
    if (path === 'tool:memorymap') return { icon: '🗺️', name: 'Memory Map' };
    if (path === 'tool:settings') return { icon: '⚙️', name: 'Settings' };

    const fileName = path.split('/').pop() || path;
    if (fileName.endsWith('.py')) return { icon: '🐍', name: fileName };
    if (fileName.endsWith('.s') || fileName.endsWith('.asm')) return { icon: '⚙️', name: fileName };
    if (fileName.endsWith('.i') || fileName.endsWith('.inc') || fileName.endsWith('.h'))
      return { icon: '📄', name: fileName };
    if (fileName.endsWith('.json')) return { icon: '🔧', name: fileName };
    return { icon: '📄', name: fileName };
  }

  function renderTabs() {
    const listEl = container.querySelector('#tabs-list-container');
    const breadcrumbsEl = container.querySelector('#editor-breadcrumbs');
    const modeHintEl = container.querySelector('#editor-mode-hint');
    const asmPreviewBtn = container.querySelector('#tabs-btn-asm-preview') as HTMLElement | null;

    if (!listEl) return;

    // Show/hide ASM Preview button if active file is Python
    if (asmPreviewBtn) {
      if (currentActiveFile.endsWith('.py') && !currentActiveFile.startsWith('tool:')) {
        asmPreviewBtn.classList.remove('hidden');
        if (isAsmPreview) {
          asmPreviewBtn.className =
            'px-2 py-0.5 rounded text-[10px] font-mono flex items-center space-x-1 border transition cursor-pointer bg-blue-600/30 text-blue-400 border-blue-500 font-bold';
        } else {
          asmPreviewBtn.className =
            'px-2 py-0.5 rounded text-[10px] font-mono flex items-center space-x-1 border transition cursor-pointer bg-studio-panel text-studio-muted border-studio-border hover:text-white';
        }
      } else {
        asmPreviewBtn.classList.add('hidden');
      }
    }

    listEl.innerHTML = currentOpenFiles
      .map((path) => {
        const isSelected = path === currentActiveFile;
        const isDirty = currentDirtyFiles.has(path);
        const { icon, name } = getFileMeta(path);

        return `
          <div class="editor-tab h-full px-3 flex items-center space-x-2 border-r border-studio-border/50 transition cursor-pointer group ${
            isSelected
              ? 'bg-studio-panel text-white font-medium border-t-2 border-t-blue-500'
              : 'bg-studio-sidebar/40 text-studio-muted hover:bg-studio-panel/50 hover:text-studio-text'
          }" data-filepath="${path}" role="tab" aria-selected="${isSelected ? 'true' : 'false'}" aria-label="Tab: ${name}">
            <span class="text-xs shrink-0">${icon}</span>
            <span class="truncate max-w-[140px] text-xs">${name}</span>
            <button class="tab-btn-close w-4 h-4 rounded hover:bg-studio-hover flex items-center justify-center text-[10px] text-studio-muted hover:text-white transition opacity-60 group-hover:opacity-100 cursor-pointer" data-closepath="${path}" aria-label="Tab ${name} schließen" title="Schließen">
              ${isDirty ? '●' : '✕'}
            </button>
          </div>
        `;
      })
      .join('');

    // Render Breadcrumbs
    if (breadcrumbsEl) {
      if (currentActiveFile.startsWith('tool:')) {
        const { icon, name } = getFileMeta(currentActiveFile);
        breadcrumbsEl.innerHTML = `<span class="text-white font-semibold flex items-center space-x-1"><span>${icon}</span><span>Tools › ${name}</span></span>`;
      } else {
        const parts = currentActiveFile.split('/').filter(Boolean);
        breadcrumbsEl.innerHTML = parts
          .map((p, idx) => {
            const isLast = idx === parts.length - 1;
            return `
              <span class="${
                isLast
                  ? 'text-white font-semibold'
                  : 'text-studio-muted hover:text-studio-text cursor-pointer'
              }">${p}</span>
              ${!isLast ? '<span class="text-studio-muted/50">›</span>' : ''}
            `;
          })
          .join('');
      }
    }

    if (modeHintEl) {
      if (currentActiveFile.startsWith('tool:')) {
        modeHintEl.textContent = 'INTERACTIVE STUDIO TOOL';
      } else if (currentActiveFile.endsWith('.py')) {
        modeHintEl.textContent = isAsmPreview ? 'PYTHON AOT (SPLIT PREVIEW)' : 'PYTHON AOT';
      } else {
        modeHintEl.textContent = 'M68K ASSEMBLY';
      }
    }

    // Tab click events
    listEl.querySelectorAll('.editor-tab').forEach((tabEl) => {
      tabEl.addEventListener('click', (e) => {
        if ((e.target as HTMLElement).closest('.tab-btn-close')) return;
        const fPath = tabEl.getAttribute('data-filepath');
        if (fPath && fPath !== currentActiveFile) {
          props.onSelectTab(fPath);
        }
      });
    });

    // Close button click
    listEl.querySelectorAll('.tab-btn-close').forEach((closeBtn) => {
      closeBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        const fPath = closeBtn.getAttribute('data-closepath');
        if (fPath) {
          props.onCloseTab(fPath);
        }
      });
    });
  }

  container.querySelector('#tabs-btn-new')?.addEventListener('click', props.onNewFile);
  container.querySelector('#tabs-btn-asm-preview')?.addEventListener('click', () => {
    if (props.onToggleAsmPreview) {
      props.onToggleAsmPreview();
    }
  });

  renderTabs();

  function updateTabs(
    openFiles: string[],
    activeFile: string,
    dirtyFiles: Set<string>,
    isAsmPreviewActive?: boolean
  ) {
    currentOpenFiles = [...openFiles];
    currentActiveFile = activeFile;
    currentDirtyFiles = new Set(dirtyFiles);
    if (isAsmPreviewActive !== undefined) isAsmPreview = isAsmPreviewActive;
    renderTabs();
  }

  return {
    element: container,
    updateTabs,
  };
}
