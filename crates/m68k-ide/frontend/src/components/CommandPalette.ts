//! VS Code Style Command Palette & Quick Open (Ctrl+P / F1) for m68k Studio.

export interface CommandItem {
  id: string;
  title: string;
  category?: string;
  shortcut?: string;
  icon?: string;
  run: () => void;
}

export interface FileItem {
  path: string;
  name: string;
  icon?: string;
}

let activePaletteOverlay: HTMLElement | null = null;

export function dismissCommandPalette() {
  if (activePaletteOverlay && document.body.contains(activePaletteOverlay)) {
    activePaletteOverlay.remove();
    activePaletteOverlay = null;
  }
}

export function showCommandPalette(options: {
  mode: 'commands' | 'files';
  commands?: CommandItem[];
  files?: FileItem[];
  onFileSelect?: (path: string) => void;
}) {
  dismissCommandPalette();

  const overlay = document.createElement('div');
  overlay.id = 'command-palette-overlay';
  overlay.className =
    'fixed inset-0 z-[100000] bg-black/60 backdrop-blur-xs flex items-start justify-center pt-16 select-none animate-fade-in';

  const isCommandMode = options.mode === 'commands';
  const placeholder = isCommandMode
    ? 'Befehl eingeben oder suchen... (z.B. Build, Run, Format)'
    : 'Datei nach Name suchen... (z.B. main.py, custom.i)';

  overlay.innerHTML = `
    <div class="bg-studio-panel/95 border border-studio-border rounded-xl w-full max-w-xl shadow-2xl overflow-hidden flex flex-col font-sans text-xs">
      <!-- Input Box -->
      <div class="p-2.5 border-b border-studio-border flex items-center space-x-2 bg-studio-bg/60">
        <span class="text-studio-muted text-sm pl-1">${isCommandMode ? '⚡' : '🔍'}</span>
        <input type="text" id="palette-input" placeholder="${placeholder}" autofocus class="flex-1 bg-transparent text-white placeholder-studio-muted/70 outline-none text-xs font-medium">
        <span class="text-[10px] text-studio-muted font-mono bg-studio-panel px-1.5 py-0.5 rounded border border-studio-border">ESC zum Schließen</span>
      </div>

      <!-- Results List -->
      <div id="palette-results" class="max-h-72 overflow-y-auto p-1 space-y-0.5">
        <!-- Rendered items -->
      </div>
    </div>
  `;

  document.body.appendChild(overlay);
  activePaletteOverlay = overlay;

  const input = overlay.querySelector('#palette-input') as HTMLInputElement;
  const resultsContainer = overlay.querySelector('#palette-results') as HTMLElement;
  let selectedIndex = 0;
  let currentFilteredList: Array<CommandItem | FileItem> = [];

  function getFileIcon(name: string): string {
    if (name.endsWith('.py')) return '🐍';
    if (name.endsWith('.s') || name.endsWith('.asm')) return '⚙️';
    if (name.endsWith('.i') || name.endsWith('.inc')) return '📄';
    return '📄';
  }

  function renderList() {
    const query = input.value.toLowerCase().trim();

    if (isCommandMode) {
      const allCmds = options.commands || [];
      currentFilteredList = allCmds.filter((c) =>
        query ? c.title.toLowerCase().includes(query) || (c.category && c.category.toLowerCase().includes(query)) : true
      );
    } else {
      const allFiles = options.files || [];
      currentFilteredList = allFiles.filter((f) =>
        query ? f.name.toLowerCase().includes(query) || f.path.toLowerCase().includes(query) : true
      );
    }

    if (selectedIndex >= currentFilteredList.length) {
      selectedIndex = Math.max(0, currentFilteredList.length - 1);
    }

    if (currentFilteredList.length === 0) {
      resultsContainer.innerHTML = `
        <div class="py-6 text-center text-studio-muted text-xs">
          Keine passenden ${isCommandMode ? 'Befehle' : 'Dateien'} gefunden
        </div>
      `;
      return;
    }

    resultsContainer.innerHTML = currentFilteredList
      .map((item, idx) => {
        const isSelected = idx === selectedIndex;

        if (isCommandMode) {
          const cmd = item as CommandItem;
          return `
            <div class="palette-item px-3 py-1.5 rounded-lg flex items-center justify-between cursor-pointer transition ${
              isSelected ? 'bg-blue-600 text-white font-semibold' : 'text-studio-text hover:bg-studio-hover'
            }" data-idx="${idx}">
              <div class="flex items-center space-x-2 truncate min-w-0">
                <span class="text-sm shrink-0">${cmd.icon || '⚡'}</span>
                ${cmd.category ? `<span class="text-[10px] opacity-70 uppercase mr-1">${cmd.category}:</span>` : ''}
                <span class="truncate">${cmd.title}</span>
              </div>
              ${cmd.shortcut ? `<span class="text-[10px] font-mono opacity-80 shrink-0 ml-2 bg-black/20 px-1.5 py-0.5 rounded">${cmd.shortcut}</span>` : ''}
            </div>
          `;
        } else {
          const file = item as FileItem;
          return `
            <div class="palette-item px-3 py-1.5 rounded-lg flex items-center justify-between cursor-pointer transition ${
              isSelected ? 'bg-blue-600 text-white font-semibold' : 'text-studio-text hover:bg-studio-hover'
            }" data-idx="${idx}">
              <div class="flex items-center space-x-2 truncate min-w-0">
                <span class="text-sm shrink-0">${file.icon || getFileIcon(file.name)}</span>
                <span class="font-medium truncate">${file.name}</span>
              </div>
              <span class="text-[10px] opacity-60 font-mono shrink-0 ml-2 truncate max-w-xs">${file.path}</span>
            </div>
          `;
        }
      })
      .join('');

    // Click handler on items
    resultsContainer.querySelectorAll('.palette-item').forEach((el) => {
      el.addEventListener('click', () => {
        const idx = parseInt(el.getAttribute('data-idx') || '0', 10);
        executeSelection(idx);
      });
    });
  }

  function executeSelection(idx: number) {
    const item = currentFilteredList[idx];
    if (!item) return;
    dismissCommandPalette();

    if (isCommandMode) {
      (item as CommandItem).run();
    } else {
      if (options.onFileSelect) {
        options.onFileSelect((item as FileItem).path);
      }
    }
  }

  input.addEventListener('input', () => {
    selectedIndex = 0;
    renderList();
  });

  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % Math.max(1, currentFilteredList.length);
      renderList();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex = (selectedIndex - 1 + currentFilteredList.length) % Math.max(1, currentFilteredList.length);
      renderList();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      executeSelection(selectedIndex);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      dismissCommandPalette();
    }
  });

  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) {
      dismissCommandPalette();
    }
  });

  input.focus();
  renderList();
}
