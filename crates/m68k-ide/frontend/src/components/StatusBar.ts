//! VS Code Style Status Bar Component for m68k Studio.

export interface StatusBarProps {
  activeProfileName: string;
  activeCpu: string;
  errorCount: number;
  warningCount: number;
  cursorLine: number;
  cursorCol: number;
  tabSize: number;
  language: string;
  cycleInfo: string;
  onOpenProblems: () => void;
  onOpenProfileSettings: () => void;
}

export function createStatusBar(props: StatusBarProps): {
  element: HTMLElement;
  updateCursor: (line: number, col: number) => void;
  updateProblemsCount: (errors: number, warnings: number) => void;
  updateProfile: (name: string, cpu: string) => void;
  updateLanguage: (lang: string) => void;
  updateCycles: (cycleInfo: string) => void;
} {
  const container = document.createElement('div');
  container.className =
    'h-6 bg-[#007acc] text-white flex items-center justify-between px-3 text-[11px] font-sans select-none shrink-0 z-30 font-medium';

  container.innerHTML = `
    <!-- Left Status Section -->
    <div class="flex items-center space-x-3">
      <!-- Remote / Studio Brand Pill -->
      <div class="flex items-center space-x-1 hover:bg-black/20 px-1.5 py-0.5 rounded cursor-pointer transition">
        <span>><</span>
        <span class="font-bold">m68k Studio</span>
      </div>

      <!-- Problems Quick Indicator -->
      <button id="sbar-btn-problems" class="flex items-center space-x-1.5 hover:bg-black/20 px-1.5 py-0.5 rounded cursor-pointer transition">
        <span class="flex items-center space-x-0.5">
          <span>⊗</span>
          <span id="sbar-err-count">${props.errorCount}</span>
        </span>
        <span class="flex items-center space-x-0.5">
          <span>⚠</span>
          <span id="sbar-warn-count">${props.warningCount}</span>
        </span>
      </button>

      <!-- Active Profile Pill -->
      <button id="sbar-btn-profile" class="flex items-center space-x-1 hover:bg-black/20 px-1.5 py-0.5 rounded cursor-pointer transition">
        <span id="sbar-profile-name">${props.activeProfileName}</span>
      </button>
    </div>

    <!-- Right Status Section -->
    <div class="flex items-center space-x-3">
      <!-- Cycle Budget & Scanlines Live Badge -->
      <div class="flex items-center space-x-1 hover:bg-black/20 px-1.5 py-0.5 rounded font-mono text-[10px]" id="sbar-cycle-info">
        ${props.cycleInfo || '⚡ ~0 Cycles'}
      </div>

      <!-- Cursor Line & Column -->
      <div class="flex items-center hover:bg-black/20 px-1.5 py-0.5 rounded font-mono" id="sbar-cursor-pos">
        Ln ${props.cursorLine}, Col ${props.cursorCol}
      </div>

      <!-- Spaces -->
      <div class="flex items-center hover:bg-black/20 px-1.5 py-0.5 rounded">
        Spaces: ${props.tabSize}
      </div>

      <!-- Encoding -->
      <div class="flex items-center hover:bg-black/20 px-1.5 py-0.5 rounded">
        UTF-8
      </div>

      <!-- Language Mode Badge -->
      <div class="flex items-center space-x-1 hover:bg-black/20 px-1.5 py-0.5 rounded font-bold cursor-pointer" id="sbar-lang-mode">
        ${props.language.toUpperCase()}
      </div>
    </div>
  `;

  container.querySelector('#sbar-btn-problems')?.addEventListener('click', props.onOpenProblems);
  container.querySelector('#sbar-btn-profile')?.addEventListener('click', props.onOpenProfileSettings);

  function updateCursor(line: number, col: number) {
    const el = container.querySelector('#sbar-cursor-pos');
    if (el) el.textContent = `Ln ${line}, Col ${col}`;
  }

  function updateProblemsCount(errors: number, warnings: number) {
    const errEl = container.querySelector('#sbar-err-count');
    const warnEl = container.querySelector('#sbar-warn-count');
    if (errEl) errEl.textContent = `${errors}`;
    if (warnEl) warnEl.textContent = `${warnings}`;
  }

  function updateProfile(name: string, _cpu: string) {
    const el = container.querySelector('#sbar-profile-name');
    if (el) el.textContent = name;
  }

  function updateLanguage(lang: string) {
    const el = container.querySelector('#sbar-lang-mode');
    if (el) el.textContent = lang === 'python' ? '🐍 PYTHON (m68k)' : '⚙️ M68K ASM';
  }

  function updateCycles(cycleInfo: string) {
    const el = container.querySelector('#sbar-cycle-info');
    if (el) el.textContent = cycleInfo;
  }

  return {
    element: container,
    updateCursor,
    updateProblemsCount,
    updateProfile,
    updateLanguage,
    updateCycles,
  };
}
