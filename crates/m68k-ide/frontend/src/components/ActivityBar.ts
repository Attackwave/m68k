//! VS Code Activity Bar Component for m68k Studio.

export type ActivityViewId = 'explorer' | 'search' | 'retro' | 'debug' | 'templates' | 'settings';

export interface ActivityBarProps {
  activeView: ActivityViewId;
  sidebarVisible: boolean;
  onViewChange: (viewId: ActivityViewId) => void;
  onToggleSidebar: () => void;
  onOpenSettings: () => void;
  onOpenTemplates: () => void;
}

export function createActivityBar(props: ActivityBarProps): {
  element: HTMLElement;
  setActiveView: (viewId: ActivityViewId, sidebarVisible: boolean) => void;
} {
  const container = document.createElement('div');
  container.className =
    'w-12 bg-studio-sidebar border-r border-studio-border flex flex-col items-center justify-between py-2 shrink-0 select-none z-20';

  let currentActive = props.activeView;
  let isSidebarVisible = props.sidebarVisible;

  container.innerHTML = `
    <!-- Top Activity Icons -->
    <div class="flex flex-col items-center space-y-1 w-full" id="act-top-group">
      <button id="act-btn-explorer" data-view="explorer" aria-label="Explorer (Ctrl+Shift+E)" title="Explorer (Ctrl+Shift+E)" class="act-item w-10 h-10 rounded-lg flex items-center justify-center text-lg transition cursor-pointer relative group">
        <span class="opacity-80 group-hover:opacity-100 group-hover:scale-110 transition">📁</span>
        <div class="indicator absolute left-0 top-2 bottom-2 w-0.5 bg-blue-500 rounded-r ${
          currentActive === 'explorer' && isSidebarVisible ? 'block' : 'hidden'
        }"></div>
      </button>

      <button id="act-btn-search" data-view="search" aria-label="Projektweite Suche (Ctrl+Shift+F)" title="Projektweite Suche (Ctrl+Shift+F)" class="act-item w-10 h-10 rounded-lg flex items-center justify-center text-lg transition cursor-pointer relative group">
        <span class="opacity-80 group-hover:opacity-100 group-hover:scale-110 transition">🔍</span>
        <div class="indicator absolute left-0 top-2 bottom-2 w-0.5 bg-blue-500 rounded-r ${
          currentActive === 'search' && isSidebarVisible ? 'block' : 'hidden'
        }"></div>
      </button>

      <button id="act-btn-retro" data-view="retro" aria-label="Retro & Hardware Studio" title="Retro & Hardware Studio (Copper, Blitter, Bitplanes)" class="act-item w-10 h-10 rounded-lg flex items-center justify-center text-lg transition cursor-pointer relative group">
        <span class="opacity-80 group-hover:opacity-100 group-hover:scale-110 transition">🎛️</span>
        <div class="indicator absolute left-0 top-2 bottom-2 w-0.5 bg-blue-500 rounded-r ${
          currentActive === 'retro' && isSidebarVisible ? 'block' : 'hidden'
        }"></div>
      </button>

      <button id="act-btn-debug" data-view="debug" aria-label="Ausführen & Emulation (Ctrl+Shift+D)" title="Ausführen & Emulation (Ctrl+Shift+D)" class="act-item w-10 h-10 rounded-lg flex items-center justify-center text-lg transition cursor-pointer relative group">
        <span class="opacity-80 group-hover:opacity-100 group-hover:scale-110 transition">▶️</span>
        <div class="indicator absolute left-0 top-2 bottom-2 w-0.5 bg-blue-500 rounded-r ${
          currentActive === 'debug' && isSidebarVisible ? 'block' : 'hidden'
        }"></div>
      </button>
    </div>

    <!-- Bottom Activity Icons -->
    <div class="flex flex-col items-center space-y-1 w-full" id="act-bottom-group">
      <button id="act-btn-templates" data-view="templates" aria-label="Projekt-Vorlagen & Wizard" title="Projekt-Vorlagen & Wizard" class="act-item w-10 h-10 rounded-lg flex items-center justify-center text-lg transition cursor-pointer relative group">
        <span class="opacity-80 group-hover:opacity-100 group-hover:scale-110 transition">📦</span>
      </button>

      <button id="act-btn-settings" data-view="settings" aria-label="Einstellungen & Profile (Ctrl+,)" title="Einstellungen & Profile (Ctrl+,)" class="act-item w-10 h-10 rounded-lg flex items-center justify-center text-lg transition cursor-pointer relative group">
        <span class="opacity-80 group-hover:opacity-100 group-hover:scale-110 transition">⚙️</span>
      </button>
    </div>
  `;

  function updateItemStyles() {
    container.querySelectorAll('.act-item').forEach((btn) => {
      const v = btn.getAttribute('data-view');
      const isSelected = v === currentActive && isSidebarVisible;
      const indicator = btn.querySelector('.indicator') as HTMLElement | null;

      if (isSelected) {
        btn.classList.add('text-white', 'bg-studio-panel/80');
        btn.classList.remove('text-studio-muted', 'hover:bg-studio-hover');
        if (indicator) indicator.classList.remove('hidden');
      } else {
        btn.classList.remove('text-white', 'bg-studio-panel/80');
        btn.classList.add('text-studio-muted', 'hover:bg-studio-hover');
        if (indicator) indicator.classList.add('hidden');
      }
    });
  }

  container.querySelectorAll('.act-item').forEach((btn) => {
    btn.addEventListener('click', () => {
      const viewId = btn.getAttribute('data-view') as ActivityViewId;

      if (viewId === 'settings') {
        props.onOpenSettings();
        return;
      }
      if (viewId === 'templates') {
        props.onOpenTemplates();
        return;
      }

      if (currentActive === viewId) {
        // Toggle sidebar visibility on re-clicking the same active icon
        isSidebarVisible = !isSidebarVisible;
        props.onToggleSidebar();
      } else {
        currentActive = viewId;
        isSidebarVisible = true;
        props.onViewChange(viewId);
      }
      updateItemStyles();
    });
  });

  updateItemStyles();

  function setActiveView(viewId: ActivityViewId, sidebarVisible: boolean) {
    currentActive = viewId;
    isSidebarVisible = sidebarVisible;
    updateItemStyles();
  }

  return {
    element: container,
    setActiveView,
  };
}
