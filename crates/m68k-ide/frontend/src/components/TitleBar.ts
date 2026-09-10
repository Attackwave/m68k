//! VS Code Style TitleBar & Top Menu Bar for m68k Studio.

import { PlatformProfile } from '../profiles';

export interface TitleBarProps {
  projectName: string;
  activeProfileId: string;
  profiles: Record<string, PlatformProfile>;
  onProfileChange: (profileId: string) => void;
  onBuild: () => void;
  onRun: () => void;
  onFormat: () => void;
  onOpenCommandPalette: (mode: 'commands' | 'files') => void;
  onToggleSidebar: () => void;
  onTogglePanel: () => void;
  onOpenSettings: () => void;
  onNewProject: () => void;
  onNewFile: () => void;
  onExportProject: () => void;
  onThemeToggle: () => void;
}

export function createTitleBar(props: TitleBarProps): {
  element: HTMLElement;
  updateActiveProfile: (profileId: string) => void;
  updateProfilesList: (profiles: Record<string, PlatformProfile>, activeId: string) => void;
  updateProjectName: (name: string) => void;
} {
  const container = document.createElement('div');
  container.className =
    'h-9 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-3 z-30 shrink-0 select-none text-xs font-sans';

  container.innerHTML = `
    <!-- Left: App Icon & Menus -->
    <div class="flex items-center space-x-1">
      <!-- App Brand Icon -->
      <div class="flex items-center space-x-1.5 mr-2 text-white font-bold tracking-wider">
        <span class="text-sm">🕹️</span>
        <span class="text-[11px] font-extrabold hidden sm:inline">m68k STUDIO</span>
      </div>

      <!-- Menu Dropdowns -->
      <div class="flex items-center space-x-0.5 text-studio-text text-xs" id="top-menus">
        <!-- File Menu -->
        <div class="relative group">
          <button class="px-2 py-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white cursor-pointer transition">
            Datei
          </button>
          <div class="absolute left-0 top-full mt-0.5 w-52 bg-studio-panel/95 backdrop-blur-md border border-studio-border rounded-xl shadow-2xl py-1 hidden group-hover:block z-50 animate-fade-in text-xs">
            <button id="menu-new-file" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Neue Datei...</span>
              <span class="text-[10px] font-mono text-studio-muted">Ctrl+N</span>
            </button>
            <button id="menu-new-project" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Neues Projekt aus Vorlage...</span>
              <span class="text-[10px] font-mono text-studio-muted">📦</span>
            </button>
            <div class="h-px bg-studio-border my-1"></div>
            <button id="menu-export-project" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Projekt exportieren...</span>
              <span class="text-[10px] font-mono text-studio-muted">💾</span>
            </button>
            <div class="h-px bg-studio-border my-1"></div>
            <button id="menu-settings" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Einstellungen & Profile...</span>
              <span class="text-[10px] font-mono text-studio-muted">Ctrl+,</span>
            </button>
          </div>
        </div>

        <!-- Edit Menu -->
        <div class="relative group">
          <button class="px-2 py-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white cursor-pointer transition">
            Bearbeiten
          </button>
          <div class="absolute left-0 top-full mt-0.5 w-48 bg-studio-panel/95 backdrop-blur-md border border-studio-border rounded-xl shadow-2xl py-1 hidden group-hover:block z-50 animate-fade-in text-xs">
            <button id="menu-format" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Dokument formatieren</span>
              <span class="text-[10px] font-mono text-studio-muted">Shift+Alt+F</span>
            </button>
          </div>
        </div>

        <!-- View Menu -->
        <div class="relative group">
          <button class="px-2 py-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white cursor-pointer transition">
            Ansicht
          </button>
          <div class="absolute left-0 top-full mt-0.5 w-56 bg-studio-panel/95 backdrop-blur-md border border-studio-border rounded-xl shadow-2xl py-1 hidden group-hover:block z-50 animate-fade-in text-xs">
            <button id="menu-cmd-pal" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Befehlspalette...</span>
              <span class="text-[10px] font-mono text-studio-muted">F1</span>
            </button>
            <button id="menu-quick-open" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Schnellsuche (Datei)...</span>
              <span class="text-[10px] font-mono text-studio-muted">Ctrl+P</span>
            </button>
            <div class="h-px bg-studio-border my-1"></div>
            <button id="menu-toggle-sidebar" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Seitenleiste umschalten</span>
              <span class="text-[10px] font-mono text-studio-muted">Ctrl+B</span>
            </button>
            <button id="menu-toggle-panel" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Konsole unten umschalten</span>
              <span class="text-[10px] font-mono text-studio-muted">Ctrl+J</span>
            </button>
            <div class="h-px bg-studio-border my-1"></div>
            <button id="menu-theme" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Theme wechseln</span>
              <span class="text-[10px] font-mono text-studio-muted">🎨</span>
            </button>
          </div>
        </div>

        <!-- Build & Run Menu -->
        <div class="relative group">
          <button class="px-2 py-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white cursor-pointer transition">
            Ausführen
          </button>
          <div class="absolute left-0 top-full mt-0.5 w-52 bg-studio-panel/95 backdrop-blur-md border border-studio-border rounded-xl shadow-2xl py-1 hidden group-hover:block z-50 animate-fade-in text-xs">
            <button id="menu-build" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Kompilieren (Build)</span>
              <span class="text-[10px] font-mono text-studio-muted">F7</span>
            </button>
            <button id="menu-run" class="w-full text-left px-3 py-1.5 hover:bg-studio-hover hover:text-white flex items-center justify-between cursor-pointer">
              <span>Starten im Emulator</span>
              <span class="text-[10px] font-mono text-studio-muted">F5</span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Center: VS Code Style Search Pill / Command Search Bar -->
    <div class="flex-1 max-w-md mx-4">
      <button id="title-search-bar" aria-label="Befehle oder Dateien suchen (Ctrl+P)" class="w-full bg-studio-panel hover:bg-studio-hover/80 border border-studio-border/70 hover:border-blue-500/50 text-studio-muted hover:text-white rounded-lg px-3 py-1 flex items-center justify-between transition cursor-pointer shadow-inner group">
        <div class="flex items-center space-x-2 truncate">
          <span class="text-xs text-studio-muted group-hover:text-blue-400">🔍</span>
          <span class="truncate text-[11px] font-medium" id="title-project-label">${props.projectName} - Befehle oder Dateien suchen...</span>
        </div>
        <span class="text-[10px] font-mono text-studio-muted bg-studio-bg/60 px-1.5 py-0.2 rounded border border-studio-border/50">Ctrl+P</span>
      </button>
    </div>

    <!-- Right: Profile, Action Controls, and Layout Toggles -->
    <div class="flex items-center space-x-2">
      <!-- Target Profile Selector -->
      <div class="flex items-center space-x-1">
        <select id="title-profile-select" aria-label="Zielprofil auswählen" class="bg-studio-panel border border-studio-border text-white text-xs font-semibold rounded-md px-2 py-0.5 outline-none focus:border-blue-500 cursor-pointer">
          ${renderProfileOptions(props.profiles, props.activeProfileId)}
        </select>
      </div>

      <!-- Quick Action Buttons -->
      <button id="title-btn-build" aria-label="Projekt kompilieren (F7)" title="Projekt kompilieren (F7)" class="px-2.5 py-0.5 bg-blue-600 hover:bg-blue-500 font-bold rounded-md text-white flex items-center space-x-1 shadow transition cursor-pointer text-xs">
        <span>🔨 Build</span>
      </button>

      <button id="title-btn-run" aria-label="Im Emulator starten (F5)" title="Im Emulator starten (F5)" class="px-2.5 py-0.5 bg-emerald-600 hover:bg-emerald-500 font-bold rounded-md text-white flex items-center space-x-1 shadow transition cursor-pointer text-xs">
        <span>▶ Run</span>
      </button>

      <div class="h-4 w-px bg-studio-border mx-1"></div>

      <!-- Layout View Toggles (Sidebar & Bottom Panel) -->
      <button id="title-toggle-sidebar" aria-label="Seitenleiste umschalten (Ctrl+B)" title="Seitenleiste umschalten (Ctrl+B)" class="p-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white transition cursor-pointer">
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="9" y1="3" x2="9" y2="21"/></svg>
      </button>
      <button id="title-toggle-panel" aria-label="Unteres Panel umschalten (Ctrl+J)" title="Unteres Panel umschalten (Ctrl+J)" class="p-1 hover:bg-studio-hover rounded text-studio-muted hover:text-white transition cursor-pointer">
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="15" x2="21" y2="15"/></svg>
      </button>
    </div>
  `;

  function renderProfileOptions(profiles: Record<string, PlatformProfile>, activeId: string): string {
    return Object.values(profiles)
      .map((p) => `<option value="${p.id}" ${p.id === activeId ? 'selected' : ''}>${p.name}</option>`)
      .join('');
  }

  const profileSelect = container.querySelector('#title-profile-select') as HTMLSelectElement;
  profileSelect.addEventListener('change', () => props.onProfileChange(profileSelect.value));

  container.querySelector('#title-btn-build')?.addEventListener('click', props.onBuild);
  container.querySelector('#title-btn-run')?.addEventListener('click', props.onRun);
  container.querySelector('#title-search-bar')?.addEventListener('click', () => props.onOpenCommandPalette('files'));

  container.querySelector('#title-toggle-sidebar')?.addEventListener('click', props.onToggleSidebar);
  container.querySelector('#title-toggle-panel')?.addEventListener('click', props.onTogglePanel);

  // Menu items click events
  container.querySelector('#menu-new-file')?.addEventListener('click', props.onNewFile);
  container.querySelector('#menu-new-project')?.addEventListener('click', props.onNewProject);
  container.querySelector('#menu-export-project')?.addEventListener('click', props.onExportProject);
  container.querySelector('#menu-settings')?.addEventListener('click', props.onOpenSettings);
  container.querySelector('#menu-format')?.addEventListener('click', props.onFormat);
  container.querySelector('#menu-cmd-pal')?.addEventListener('click', () => props.onOpenCommandPalette('commands'));
  container.querySelector('#menu-quick-open')?.addEventListener('click', () => props.onOpenCommandPalette('files'));
  container.querySelector('#menu-toggle-sidebar')?.addEventListener('click', props.onToggleSidebar);
  container.querySelector('#menu-toggle-panel')?.addEventListener('click', props.onTogglePanel);
  container.querySelector('#menu-theme')?.addEventListener('click', props.onThemeToggle);
  container.querySelector('#menu-build')?.addEventListener('click', props.onBuild);
  container.querySelector('#menu-run')?.addEventListener('click', props.onRun);

  return {
    element: container,
    updateActiveProfile: (id: string) => {
      if (profileSelect) profileSelect.value = id;
    },
    updateProfilesList: (profiles: Record<string, PlatformProfile>, activeId: string) => {
      if (profileSelect) profileSelect.innerHTML = renderProfileOptions(profiles, activeId);
    },
    updateProjectName: (name: string) => {
      const label = container.querySelector('#title-project-label');
      if (label) label.textContent = `${name} - Befehle oder Dateien suchen...`;
    },
  };
}
