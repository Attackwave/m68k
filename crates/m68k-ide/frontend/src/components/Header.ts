//! Header Bar Component with Toolbar Controls and Target Profile Selector.

import { PlatformProfile } from '../profiles';

export interface HeaderProps {
  projectName: string;
  activeProfileId: string;
  profiles: Record<string, PlatformProfile>;
  onProfileChange: (profileId: string) => void;
  onBuild: () => void;
  onRun: () => void;
  onOpenSettings: () => void;
  onThemeToggle: () => void;
}

export function createHeader(props: HeaderProps): {
  element: HTMLElement;
  updateActiveProfile: (profileId: string) => void;
  updateProfilesList: (profiles: Record<string, PlatformProfile>, activeId: string) => void;
  updateProjectName: (name: string) => void;
} {
  const header = document.createElement('header');
  header.className =
    'h-12 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-4 z-10 shrink-0 select-none';

  header.innerHTML = `
    <!-- Left: Branding & Project Meta -->
    <div class="flex items-center space-x-3">
      <div class="flex items-center space-x-2">
        <svg class="w-6 h-6 text-studio-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/>
        </svg>
        <span class="font-bold tracking-wider text-base text-white">m68k STUDIO</span>
        <span class="text-xs px-2 py-0.5 rounded bg-studio-panel text-blue-400 font-semibold border border-studio-border" id="header-project-name">${props.projectName}</span>
      </div>

      <div class="h-5 w-px bg-studio-border"></div>

      <!-- Target Profile Selector -->
      <div class="flex items-center space-x-1.5 text-xs">
        <span class="text-studio-muted font-semibold">Profil:</span>
        <select id="profile-selector" class="bg-studio-panel border border-studio-border text-white text-xs font-semibold rounded px-2.5 py-1 outline-none focus:border-blue-500 cursor-pointer shadow-sm">
          ${renderProfileOptions(props.profiles, props.activeProfileId)}
        </select>
      </div>
    </div>

    <!-- Center / Right Actions: Clean Studio Toolbar -->
    <div class="flex items-center space-x-2">
      <!-- Build Action -->
      <button id="btn-build" title="Projekt kompilieren & assemblieren (F7)" class="px-3.5 py-1.5 text-xs bg-blue-600 hover:bg-blue-500 font-bold rounded-lg text-white flex items-center space-x-1.5 shadow transition cursor-pointer">
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>
        <span>Build (F7)</span>
      </button>

      <!-- Run Action -->
      <button id="btn-run" title="Im Emulator starten (F5)" class="px-3.5 py-1.5 text-xs bg-emerald-600 hover:bg-emerald-500 font-bold rounded-lg text-white flex items-center space-x-1.5 shadow transition cursor-pointer">
        <svg class="w-3.5 h-3.5 fill-current" viewBox="0 0 24 24"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        <span>Run (F5)</span>
      </button>

      <div class="h-5 w-px bg-studio-border mx-1"></div>

      <!-- Settings Quick Link -->
      <button id="btn-settings-quick" title="Einstellungen & Profile öffnen" class="px-2.5 py-1.5 text-xs bg-studio-panel hover:bg-studio-hover rounded-lg text-studio-text hover:text-white border border-studio-border transition cursor-pointer flex items-center space-x-1">
        <span>⚙️</span>
        <span class="font-semibold">Einstellungen</span>
      </button>

      <!-- Theme Switcher -->
      <button id="btn-theme" title="Theme wechseln (Dark / Amiga Retro)" class="p-1.5 text-xs bg-studio-panel hover:bg-studio-hover rounded-lg text-studio-muted hover:text-white border border-studio-border transition cursor-pointer">
        🎨
      </button>
    </div>
  `;

  function renderProfileOptions(profiles: Record<string, PlatformProfile>, activeId: string): string {
    return Object.values(profiles)
      .map(
        (p) => `
      <option value="${p.id}" ${p.id === activeId ? 'selected' : ''}>${p.name}</option>
    `
      )
      .join('');
  }

  const profileSelect = header.querySelector('#profile-selector') as HTMLSelectElement;

  profileSelect.addEventListener('change', () => props.onProfileChange(profileSelect.value));

  header.querySelector('#btn-build')?.addEventListener('click', props.onBuild);
  header.querySelector('#btn-run')?.addEventListener('click', props.onRun);
  header.querySelector('#btn-settings-quick')?.addEventListener('click', props.onOpenSettings);
  header.querySelector('#btn-theme')?.addEventListener('click', props.onThemeToggle);

  function updateActiveProfile(profileId: string) {
    if (profileSelect) profileSelect.value = profileId;
  }

  function updateProfilesList(profiles: Record<string, PlatformProfile>, activeId: string) {
    if (profileSelect) {
      profileSelect.innerHTML = renderProfileOptions(profiles, activeId);
    }
  }

  function updateProjectName(name: string) {
    const nameEl = header.querySelector('#header-project-name');
    if (nameEl) nameEl.textContent = name;
  }

  return {
    element: header,
    updateActiveProfile,
    updateProfilesList,
    updateProjectName,
  };
}
