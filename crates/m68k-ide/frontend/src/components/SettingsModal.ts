//! Interactive Settings Modal for Emulator, Assembler, and Editor Preferences.

import { api, EmulatorProfile } from '../api';

export interface AppSettings {
  emulatorProfile: string; // 'fsuae' | 'winuae' | 'blastem' | 'hatari' | 'custom'
  emulatorPath: string;
  kickstartRomPath: string;
  emulatorExtraArgs: string;
  defaultOrigin: string;
  defaultOutputFormat: string;
  editorFontSize: number;
  editorTabSize: number;
  minimapEnabled: boolean;
  theme: string;
}

export const DEFAULT_SETTINGS: AppSettings = {
  emulatorProfile: 'fsuae',
  emulatorPath: '',
  kickstartRomPath: '',
  emulatorExtraArgs: '',
  defaultOrigin: '$0000',
  defaultOutputFormat: 'adf',
  editorFontSize: 13,
  editorTabSize: 4,
  minimapEnabled: true,
  theme: 'dark',
};

export function loadSettings(): AppSettings {
  try {
    const saved = localStorage.getItem('m68k_studio_settings');
    if (saved) {
      return { ...DEFAULT_SETTINGS, ...JSON.parse(saved) };
    }
  } catch {
    // fallback
  }
  return DEFAULT_SETTINGS;
}

export function saveSettings(settings: AppSettings) {
  try {
    localStorage.setItem('m68k_studio_settings', JSON.stringify(settings));
  } catch {
    // fallback
  }
}

export function createSettingsModal(
  currentSettings: AppSettings,
  onSave: (newSettings: AppSettings) => void
): { element: HTMLElement; open: () => void; close: () => void } {
  const modalContainer = document.createElement('div');
  modalContainer.className = 'fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 hidden select-none';

  modalContainer.innerHTML = `
    <div class="w-[620px] max-h-[85vh] bg-studio-sidebar border border-studio-border rounded-xl shadow-2xl flex flex-col overflow-hidden text-xs font-sans">
      <!-- Modal Header -->
      <div class="h-12 bg-studio-panel border-b border-studio-border flex items-center justify-between px-4 shrink-0">
        <div class="flex items-center space-x-2">
          <span class="text-base">⚙️</span>
          <span class="font-bold text-white tracking-wide text-sm">m68k Studio Settings</span>
        </div>
        <button id="btn-close-settings" class="p-1 text-studio-muted hover:text-white rounded hover:bg-studio-hover transition cursor-pointer text-sm">
          ✕
        </button>
      </div>

      <!-- Tabs Header -->
      <div class="h-9 bg-studio-bg border-b border-studio-border flex items-center px-4 space-x-2 shrink-0">
        <button id="tab-settings-emu" class="px-3 py-1 font-bold text-white bg-studio-panel border border-studio-border rounded cursor-pointer">
          🎮 Emulator Setup
        </button>
        <button id="tab-settings-asm" class="px-3 py-1 font-medium text-studio-muted hover:text-white transition cursor-pointer">
          🔨 Assembler & Build
        </button>
        <button id="tab-settings-editor" class="px-3 py-1 font-medium text-studio-muted hover:text-white transition cursor-pointer">
          🎨 Editor & Appearance
        </button>
      </div>

      <!-- Tab 1: Emulator Setup -->
      <div id="view-settings-emu" class="flex-1 p-5 space-y-4 overflow-y-auto">
        <div class="space-y-1.5">
          <label class="font-bold text-white">Active Emulator Profile</label>
          <select id="setting-emu-profile" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none focus:border-blue-500">
            <option value="fsuae">FS-UAE (Amiga 500 / 1200 / 4000)</option>
            <option value="winuae">WinUAE (Windows Amiga Emulator)</option>
            <option value="blastem">BlastEm (Sega Mega Drive / Genesis)</option>
            <option value="hatari">Hatari (Atari ST / STE / TT / Falcon)</option>
            <option value="custom">Custom / Other Emulator</option>
          </select>
          <div id="emu-detect-status" class="text-[11px] text-studio-muted flex items-center space-x-1.5">
            <span>Checking system PATH for emulator...</span>
          </div>
        </div>

        <div class="space-y-1.5">
          <label class="font-bold text-white flex justify-between">
            <span>Executable Path (Optional / Custom)</span>
            <span class="text-studio-muted font-normal text-[11px]">Leave blank to use system PATH</span>
          </label>
          <input type="text" id="setting-emu-path" placeholder="e.g. C:\\WinUAE\\winuae64.exe or /usr/bin/fs-uae" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-[11px] focus:border-blue-500">
        </div>

        <div class="space-y-1.5">
          <label class="font-bold text-white flex justify-between">
            <span>Kickstart / System ROM Path (Optional)</span>
            <span class="text-studio-muted font-normal text-[11px]">Amiga kick13.rom / kick31.rom or Atari tos.img</span>
          </label>
          <input type="text" id="setting-rom-path" placeholder="e.g. /home/user/roms/kick31.rom or C:\\ROMs\\kick13.rom" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-[11px] focus:border-blue-500">
        </div>

        <div class="space-y-1.5">
          <label class="font-bold text-white flex justify-between">
            <span>Additional CLI Arguments</span>
            <span class="text-studio-muted font-normal text-[11px]">Use {FILE} placeholder</span>
          </label>
          <input type="text" id="setting-emu-args" placeholder="e.g. --fullscreen=0 --chip_memory=1024" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-[11px] focus:border-blue-500">
        </div>
      </div>

      <!-- Tab 2: Assembler & Build -->
      <div id="view-settings-asm" class="flex-1 p-5 space-y-4 overflow-y-auto hidden">
        <div class="grid grid-cols-2 gap-4">
          <div class="space-y-1.5">
            <label class="font-bold text-white">Default Origin (ORG Address)</label>
            <input type="text" id="setting-origin" value="$0000" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-xs focus:border-blue-500">
          </div>
          <div class="space-y-1.5">
            <label class="font-bold text-white">Default Build Output</label>
            <select id="setting-output-fmt" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none focus:border-blue-500">
              <option value="adf">Amiga Floppy Disk Image (.adf)</option>
              <option value="hunk">AmigaDOS Hunk Executable</option>
              <option value="bin">Raw 68000 Binary (.bin)</option>
              <option value="srec">Motorola S-Record (.srec)</option>
              <option value="hex">Intel Hex (.hex)</option>
            </select>
          </div>
        </div>

        <div class="p-3 bg-studio-panel border border-studio-border rounded-lg space-y-2 text-studio-muted text-[11px]">
          <div class="font-bold text-white">Assembler Features Enabled:</div>
          <div>✅ Automatic Short/Word/Long Branch Relaxation</div>
          <div>✅ 68000 through 68060 Central Architecture Gating</div>
          <div>✅ Full AmigaOS LVO Hardware & Library Function Resolution</div>
        </div>
      </div>

      <!-- Tab 3: Editor & Appearance -->
      <div id="view-settings-editor" class="flex-1 p-5 space-y-4 overflow-y-auto hidden">
        <div class="grid grid-cols-2 gap-4">
          <div class="space-y-1.5">
            <label class="font-bold text-white">IDE Theme</label>
            <select id="setting-theme" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none focus:border-blue-500">
              <option value="dark">Studio Dark (Modern)</option>
              <option value="amiga">Amiga Workbench 1.3 (Retro Blue)</option>
            </select>
          </div>
          <div class="space-y-1.5">
            <label class="font-bold text-white">Editor Font Size</label>
            <select id="setting-font-size" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none focus:border-blue-500">
              <option value="12">12 px</option>
              <option value="13">13 px (Default)</option>
              <option value="14">14 px</option>
              <option value="16">16 px</option>
            </select>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-4">
          <div class="space-y-1.5">
            <label class="font-bold text-white">Indentation (Tab Size)</label>
            <select id="setting-tab-size" class="w-full bg-studio-panel border border-studio-border text-white px-3 py-1.5 rounded outline-none focus:border-blue-500">
              <option value="2">2 Spaces</option>
              <option value="4" selected>4 Spaces</option>
              <option value="8">8 Spaces (Motorola Standard)</option>
            </select>
          </div>
          <div class="flex items-center space-x-2 pt-6">
            <input type="checkbox" id="setting-minimap" class="rounded bg-studio-panel border-studio-border text-blue-500 cursor-pointer">
            <label for="setting-minimap" class="text-white font-medium cursor-pointer">Show Code Minimap</label>
          </div>
        </div>
      </div>

      <!-- Footer Buttons -->
      <div class="h-14 bg-studio-panel border-t border-studio-border flex items-center justify-end px-4 space-x-2 shrink-0">
        <button id="btn-cancel-settings" class="px-4 py-1.5 rounded text-studio-text hover:bg-studio-hover border border-studio-border transition cursor-pointer">
          Cancel
        </button>
        <button id="btn-save-settings" class="px-5 py-1.5 rounded bg-blue-600 hover:bg-blue-500 text-white font-semibold shadow transition cursor-pointer">
          Save Settings
        </button>
      </div>
    </div>
  `;

  const btnClose = modalContainer.querySelector('#btn-close-settings') as HTMLElement;
  const btnCancel = modalContainer.querySelector('#btn-cancel-settings') as HTMLElement;
  const btnSave = modalContainer.querySelector('#btn-save-settings') as HTMLElement;

  const tabEmu = modalContainer.querySelector('#tab-settings-emu') as HTMLElement;
  const tabAsm = modalContainer.querySelector('#tab-settings-asm') as HTMLElement;
  const tabEditor = modalContainer.querySelector('#tab-settings-editor') as HTMLElement;

  const viewEmu = modalContainer.querySelector('#view-settings-emu') as HTMLElement;
  const viewAsm = modalContainer.querySelector('#view-settings-asm') as HTMLElement;
  const viewEditor = modalContainer.querySelector('#view-settings-editor') as HTMLElement;

  function switchTab(activeTab: HTMLElement, activeView: HTMLElement) {
    [tabEmu, tabAsm, tabEditor].forEach((t) => {
      t.className = 'px-3 py-1 font-medium text-studio-muted hover:text-white transition cursor-pointer';
    });
    [viewEmu, viewAsm, viewEditor].forEach((v) => v.classList.add('hidden'));

    activeTab.className = 'px-3 py-1 font-bold text-white bg-studio-panel border border-studio-border rounded cursor-pointer';
    activeView.classList.remove('hidden');
  }

  tabEmu.addEventListener('click', () => switchTab(tabEmu, viewEmu));
  tabAsm.addEventListener('click', () => switchTab(tabAsm, viewAsm));
  tabEditor.addEventListener('click', () => switchTab(tabEditor, viewEditor));

  const profileSelect = modalContainer.querySelector('#setting-emu-profile') as HTMLSelectElement;
  const pathInput = modalContainer.querySelector('#setting-emu-path') as HTMLInputElement;
  const romInput = modalContainer.querySelector('#setting-rom-path') as HTMLInputElement;
  const argsInput = modalContainer.querySelector('#setting-emu-args') as HTMLInputElement;

  const originInput = modalContainer.querySelector('#setting-origin') as HTMLInputElement;
  const outputFmtSelect = modalContainer.querySelector('#setting-output-fmt') as HTMLSelectElement;

  const themeSelect = modalContainer.querySelector('#setting-theme') as HTMLSelectElement;
  const fontSizeSelect = modalContainer.querySelector('#setting-font-size') as HTMLSelectElement;
  const tabSizeSelect = modalContainer.querySelector('#setting-tab-size') as HTMLSelectElement;
  const minimapCheck = modalContainer.querySelector('#setting-minimap') as HTMLInputElement;

  async function checkEmulators() {
    const statusEl = modalContainer.querySelector('#emu-detect-status');
    if (!statusEl) return;

    const detected: EmulatorProfile[] = await api.detectEmulators();
    const currentSelected = profileSelect.value;
    const foundProfile = detected.find((p) => p.id === currentSelected);

    if (foundProfile && foundProfile.available) {
      statusEl.innerHTML = `<span class="text-emerald-400 font-bold">🟢 '${foundProfile.executable_path}' detected in system PATH</span>`;
    } else {
      statusEl.innerHTML = `<span class="text-amber-400 font-medium">🟡 Executable not found in system PATH. Specify executable path below if installed.</span>`;
    }
  }

  profileSelect.addEventListener('change', checkEmulators);

  function populate(settings: AppSettings) {
    profileSelect.value = settings.emulatorProfile;
    pathInput.value = settings.emulatorPath;
    romInput.value = settings.kickstartRomPath;
    argsInput.value = settings.emulatorExtraArgs;

    originInput.value = settings.defaultOrigin;
    outputFmtSelect.value = settings.defaultOutputFormat;

    themeSelect.value = settings.theme;
    fontSizeSelect.value = `${settings.editorFontSize}`;
    tabSizeSelect.value = `${settings.editorTabSize}`;
    minimapCheck.checked = settings.minimapEnabled;

    checkEmulators();
  }

  function open() {
    populate(currentSettings);
    modalContainer.classList.remove('hidden');
  }

  function close() {
    modalContainer.classList.add('hidden');
  }

  btnClose.addEventListener('click', close);
  btnCancel.addEventListener('click', close);

  btnSave.addEventListener('click', () => {
    const updated: AppSettings = {
      emulatorProfile: profileSelect.value,
      emulatorPath: pathInput.value.trim(),
      kickstartRomPath: romInput.value.trim(),
      emulatorExtraArgs: argsInput.value.trim(),
      defaultOrigin: originInput.value.trim() || '$0000',
      defaultOutputFormat: outputFmtSelect.value,
      theme: themeSelect.value,
      editorFontSize: parseInt(fontSizeSelect.value, 10) || 13,
      editorTabSize: parseInt(tabSizeSelect.value, 10) || 4,
      minimapEnabled: minimapCheck.checked,
    };
    saveSettings(updated);
    onSave(updated);
    close();
  });

  return { element: modalContainer, open, close };
}
