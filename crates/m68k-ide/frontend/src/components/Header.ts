//! Header Bar Component with Toolbar Controls.

export interface HeaderProps {
  projectName: string;
  targetCpu: string;
  onCpuChange: (cpu: string) => void;
  onBuild: () => void;
  onRun: () => void;
  onFormat: () => void;
  onNewProject: (template: string) => void;
  onThemeToggle: () => void;
}

export function createHeader(props: HeaderProps): HTMLElement {
  const header = document.createElement('header');
  header.className = 'h-12 bg-studio-sidebar border-b border-studio-border flex items-center justify-between px-4 z-10 shrink-0';

  header.innerHTML = `
    <div class="flex items-center space-x-4">
      <div class="flex items-center space-x-2">
        <svg class="w-6 h-6 text-studio-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/>
        </svg>
        <span class="font-bold tracking-wider text-base text-white">m68k STUDIO</span>
        <span class="text-xs px-2 py-0.5 rounded bg-studio-panel text-studio-muted border border-studio-border" id="header-project-name">${props.projectName}</span>
      </div>

      <div class="h-5 w-px bg-studio-border"></div>

      <!-- New Project Template Menu -->
      <div class="relative group">
        <button class="px-2.5 py-1 text-xs bg-studio-panel hover:bg-studio-hover rounded text-studio-text flex items-center space-x-1 border border-studio-border transition">
          <span>✨ New Template</span>
          <svg class="w-3 h-3 text-studio-muted" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path d="M19 9l-7 7-7-7"/></svg>
        </button>
        <div class="absolute left-0 top-full mt-1 w-48 bg-studio-panel border border-studio-border rounded shadow-xl py-1 hidden group-hover:block z-50">
          <button class="w-full text-left px-3 py-1.5 text-xs text-studio-text hover:bg-studio-hover flex items-center space-x-2" data-template="amiga500">
            <span>🕹️ Amiga 500 Demo (ADF)</span>
          </button>
          <button class="w-full text-left px-3 py-1.5 text-xs text-studio-text hover:bg-studio-hover flex items-center space-x-2" data-template="megadrive">
            <span>🎮 Sega Mega Drive ROM</span>
          </button>
          <button class="w-full text-left px-3 py-1.5 text-xs text-studio-text hover:bg-studio-hover flex items-center space-x-2" data-template="baremetal">
            <span>⚡ Bare Metal 68000 Binary</span>
          </button>
        </div>
      </div>

      <!-- CPU Selector -->
      <div class="flex items-center space-x-1 text-xs">
        <span class="text-studio-muted">CPU:</span>
        <select id="cpu-selector" class="bg-studio-panel border border-studio-border text-white text-xs rounded px-2 py-1 outline-none focus:border-studio-accent">
          <option value="68000" ${props.targetCpu === '68000' ? 'selected' : ''}>68000 (Amiga/MD/ST)</option>
          <option value="68010" ${props.targetCpu === '68010' ? 'selected' : ''}>68010</option>
          <option value="68020" ${props.targetCpu === '68020' ? 'selected' : ''}>68020 (Amiga 1200)</option>
          <option value="68030" ${props.targetCpu === '68030' ? 'selected' : ''}>68030 (Amiga 3000)</option>
          <option value="68040" ${props.targetCpu === '68040' ? 'selected' : ''}>68040 (Amiga 4000)</option>
          <option value="68060" ${props.targetCpu === '68060' ? 'selected' : ''}>68060</option>
        </select>
      </div>
    </div>

    <!-- Center / Right Actions -->
    <div class="flex items-center space-x-2">
      <button id="btn-format" title="Format Assembly Source (Ctrl+Shift+I)" class="px-2.5 py-1 text-xs bg-studio-panel hover:bg-studio-hover rounded text-studio-text flex items-center space-x-1 border border-studio-border transition">
        <svg class="w-3.5 h-3.5 text-studio-muted" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path d="M4 6h16M4 12h16M4 18h16"/></svg>
        <span>Format</span>
      </button>

      <button id="btn-build" title="Assemble Project (F7)" class="px-3 py-1 text-xs bg-blue-600 hover:bg-blue-500 font-medium rounded text-white flex items-center space-x-1.5 shadow transition">
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>
        <span>Build (F7)</span>
      </button>

      <button id="btn-run" title="Run in Emulator (F5)" class="px-3 py-1 text-xs bg-emerald-600 hover:bg-emerald-500 font-medium rounded text-white flex items-center space-x-1.5 shadow transition">
        <svg class="w-3.5 h-3.5 fill-current" viewBox="0 0 24 24"><polygon points="5 3 19 12 5 21 5 3"/></svg>
        <span>Run (F5)</span>
      </button>

      <div class="h-5 w-px bg-studio-border mx-1"></div>

      <!-- Theme Switcher -->
      <button id="btn-theme" title="Toggle Amiga / Dark Studio Theme" class="p-1.5 text-xs bg-studio-panel hover:bg-studio-hover rounded text-studio-muted hover:text-white border border-studio-border transition">
        🎨
      </button>
    </div>
  `;

  // Event wiring
  const cpuSelect = header.querySelector('#cpu-selector') as HTMLSelectElement;
  cpuSelect.addEventListener('change', () => props.onCpuChange(cpuSelect.value));

  header.querySelector('#btn-build')?.addEventListener('click', props.onBuild);
  header.querySelector('#btn-run')?.addEventListener('click', props.onRun);
  header.querySelector('#btn-format')?.addEventListener('click', props.onFormat);
  header.querySelector('#btn-theme')?.addEventListener('click', props.onThemeToggle);

  header.querySelectorAll('[data-template]').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      const template = (e.currentTarget as HTMLElement).getAttribute('data-template');
      if (template) props.onNewProject(template);
    });
  });

  return header;
}
