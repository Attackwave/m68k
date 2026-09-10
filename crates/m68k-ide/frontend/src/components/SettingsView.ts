import {
  PlatformProfile,
  loadAllProfiles,
  saveAllProfiles,
  BUILTIN_PROFILES,
} from '../profiles';
import { api, EmulatorProfile } from '../api';
import { showToast, showConfirmDialog, showPromptDialog } from './NotificationSystem';

export interface AppGeneralSettings {
  defaultOrigin: string;
  defaultOutputFormat: string;
  editorFontSize: number;
  editorTabSize: number;
  minimapEnabled: boolean;
  theme: string;
}

export const DEFAULT_GENERAL_SETTINGS: AppGeneralSettings = {
  defaultOrigin: '$0000',
  defaultOutputFormat: 'adf',
  editorFontSize: 13,
  editorTabSize: 4,
  minimapEnabled: true,
  theme: 'dark',
};

const GENERAL_SETTINGS_KEY = 'm68k_studio_general_settings';

export function loadGeneralSettings(): AppGeneralSettings {
  try {
    const raw = localStorage.getItem(GENERAL_SETTINGS_KEY);
    if (raw) {
      return { ...DEFAULT_GENERAL_SETTINGS, ...JSON.parse(raw) };
    }
  } catch {
    // fallback
  }
  return DEFAULT_GENERAL_SETTINGS;
}

export function saveGeneralSettings(s: AppGeneralSettings) {
  try {
    localStorage.setItem(GENERAL_SETTINGS_KEY, JSON.stringify(s));
  } catch {
    // fallback
  }
}

export function createSettingsView(
  activeProfileId: string,
  onProfileActivated: (profile: PlatformProfile) => void,
  onGeneralSettingsChanged: (s: AppGeneralSettings) => void
): {
  element: HTMLElement;
  refresh: () => void;
} {
  const container = document.createElement('div');
  container.className = 'flex-1 flex bg-studio-bg overflow-hidden text-xs font-sans select-none';

  container.innerHTML = `
    <!-- Left Navigation Sidebar -->
    <div class="w-60 bg-studio-sidebar border-r border-studio-border flex flex-col shrink-0 p-3 space-y-2">
      <div class="text-[11px] font-bold text-studio-muted uppercase tracking-wider px-2 py-1">
        ⚙️ Studio Einstellungen
      </div>

      <nav class="space-y-1">
        <button id="nav-profiles" class="w-full text-left px-3 py-2 rounded-lg font-bold text-white bg-studio-panel border border-studio-border flex items-center space-x-2.5 transition cursor-pointer">
          <span>🎯</span>
          <span>Profile verwalten</span>
        </button>
        <button id="nav-emulators" class="w-full text-left px-3 py-2 rounded-lg font-medium text-studio-muted hover:text-white hover:bg-studio-hover flex items-center space-x-2.5 transition cursor-pointer">
          <span>🎮</span>
          <span>Emulatoren & Downloads</span>
        </button>
        <button id="nav-assembler" class="w-full text-left px-3 py-2 rounded-lg font-medium text-studio-muted hover:text-white hover:bg-studio-hover flex items-center space-x-2.5 transition cursor-pointer">
          <span>🔨</span>
          <span>Assembler & Build</span>
        </button>
        <button id="nav-editor" class="w-full text-left px-3 py-2 rounded-lg font-medium text-studio-muted hover:text-white hover:bg-studio-hover flex items-center space-x-2.5 transition cursor-pointer">
          <span>🎨</span>
          <span>Editor & Aussehen</span>
        </button>
      </nav>
    </div>

    <!-- Right Main Content Area -->
    <div class="flex-1 flex overflow-hidden">
      <!-- VIEW 1: Profiles Management -->
      <div id="section-profiles" class="flex-1 flex overflow-hidden">
        <!-- Sub-sidebar: Profile List -->
        <div class="w-72 border-r border-studio-border flex flex-col bg-studio-sidebar/40 overflow-hidden shrink-0">
          <div class="p-3 border-b border-studio-border flex items-center justify-between">
            <span class="font-bold text-white text-xs">Verfügbare Profile</span>
            <button id="btn-add-profile" class="px-2 py-1 bg-blue-600 hover:bg-blue-500 text-white rounded text-[11px] font-semibold transition cursor-pointer flex items-center space-x-1">
              <span>➕ Neu</span>
            </button>
          </div>
          <div class="flex-1 overflow-y-auto p-2 space-y-1" id="profiles-list-container">
            <!-- Dynamically populated -->
          </div>
        </div>

        <!-- Right Sub-Pane: Profile Editor Form -->
        <div class="flex-1 overflow-y-auto p-6 space-y-6" id="profile-editor-container">
          <!-- Dynamically populated based on selected profile -->
        </div>
      </div>

      <!-- VIEW 2: Global Emulators Setup & Auto-Download -->
      <div id="section-emulators" class="flex-1 overflow-y-auto p-8 space-y-6 hidden">
        <div class="border-b border-studio-border pb-3">
          <h2 class="text-sm font-bold text-white tracking-wide">🎮 EMULATOR SETUP & 1-KLICK DOWNLOADS</h2>
          <p class="text-studio-muted text-xs">Lade portable Emulatoren (Mega Drive, Atari ST, Amiga) und freie Open-Source ROMs direkt mit einem Klick herunter.</p>
        </div>

        <div class="space-y-4" id="emu-list-container">
          <!-- Dynamically populated -->
        </div>
      </div>

      <!-- VIEW 3: Assembler & Build Settings -->
      <div id="section-assembler" class="flex-1 overflow-y-auto p-8 space-y-6 hidden">
        <div class="border-b border-studio-border pb-3">
          <h2 class="text-sm font-bold text-white tracking-wide">🔨 ASSEMBLER & BUILD OPTIONEN</h2>
          <p class="text-studio-muted text-xs">Standardwerte für Code-Generierung, Startadressen und Ausgabeformate.</p>
        </div>

        <div class="max-w-xl space-y-4">
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
            <div class="font-bold text-white">Standard Ausgabeformat</div>
            <select id="gen-output-fmt" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-2 rounded outline-none focus:border-blue-500">
              <option value="adf">Amiga Floppy Disk Image (.adf)</option>
              <option value="hunk">AmigaDOS Hunk Executable</option>
              <option value="bin">Raw Binary (.bin)</option>
              <option value="srec">Motorola S-Record (.srec)</option>
              <option value="hex">Intel Hex (.hex)</option>
            </select>
          </div>

          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-2 text-studio-muted text-[11px]">
            <div class="font-bold text-white">Assembler-Fähigkeiten (m68k-core & m68k-asm):</div>
            <div>✅ Automatische Branch-Relaxation (Short / Word / Long Bcc)</div>
            <div>✅ Volle 68000 bis 68060 CPU-Gate-Validierung</div>
            <div>✅ Echte DC.X Extended Float & Packed BCD Konvertierung</div>
            <div>✅ Integrierte LVO-Bibliotheksnamensauflösung</div>
          </div>
        </div>
      </div>

      <!-- VIEW 4: Editor & Appearance -->
      <div id="section-editor" class="flex-1 overflow-y-auto p-8 space-y-6 hidden">
        <div class="border-b border-studio-border pb-3">
          <h2 class="text-sm font-bold text-white tracking-wide">🎨 EDITOR & ERSCHEINUNGSBILD</h2>
          <p class="text-studio-muted text-xs">Passe Themes, Schriftgrößen und Code-Ansichten nach deinen Wünschen an.</p>
        </div>

        <div class="max-w-xl space-y-4">
          <div class="grid grid-cols-2 gap-4">
            <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-2">
              <label class="font-bold text-white">Studio Theme</label>
              <select id="gen-theme" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-2 rounded outline-none focus:border-blue-500">
                <option value="dark">Studio Dark (Modern)</option>
                <option value="amiga">Amiga Workbench 1.3 Blue (Retro)</option>
              </select>
            </div>

            <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-2">
              <label class="font-bold text-white">Editor Schriftgröße</label>
              <select id="gen-font-size" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-2 rounded outline-none focus:border-blue-500">
                <option value="12">12 px</option>
                <option value="13">13 px (Standard)</option>
                <option value="14">14 px</option>
                <option value="16">16 px</option>
              </select>
            </div>
          </div>

          <div class="grid grid-cols-2 gap-4">
            <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-2">
              <label class="font-bold text-white">Tab Einrückung</label>
              <select id="gen-tab-size" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-2 rounded outline-none focus:border-blue-500">
                <option value="2">2 Leerzeichen</option>
                <option value="4" selected>4 Leerzeichen</option>
                <option value="8">8 Leerzeichen (Motorola Standard)</option>
              </select>
            </div>

            <div class="p-4 bg-studio-panel border border-studio-border rounded-xl flex items-center justify-between">
              <div>
                <div class="font-bold text-white">Code Minimap</div>
                <div class="text-[10px] text-studio-muted">Vorschau-Leiste rechts im Editor</div>
              </div>
              <input type="checkbox" id="gen-minimap" class="rounded text-blue-500 w-4 h-4 cursor-pointer">
            </div>
          </div>
        </div>
      </div>
    </div>
  `;

  // State
  let allProfiles = loadAllProfiles();
  let selectedProfileId = activeProfileId;
  let generalSettings = loadGeneralSettings();

  // Navigation wiring
  const navProfiles = container.querySelector('#nav-profiles') as HTMLElement;
  const navEmulators = container.querySelector('#nav-emulators') as HTMLElement;
  const navAssembler = container.querySelector('#nav-assembler') as HTMLElement;
  const navEditor = container.querySelector('#nav-editor') as HTMLElement;

  const secProfiles = container.querySelector('#section-profiles') as HTMLElement;
  const secEmulators = container.querySelector('#section-emulators') as HTMLElement;
  const secAssembler = container.querySelector('#section-assembler') as HTMLElement;
  const secEditor = container.querySelector('#section-editor') as HTMLElement;

  const navButtons = [navProfiles, navEmulators, navAssembler, navEditor];
  const sections = [secProfiles, secEmulators, secAssembler, secEditor];

  function switchSection(btn: HTMLElement, sec: HTMLElement) {
    navButtons.forEach((b) => {
      b.className =
        'w-full text-left px-3 py-2 rounded-lg font-medium text-studio-muted hover:text-white hover:bg-studio-hover flex items-center space-x-2.5 transition cursor-pointer';
    });
    sections.forEach((s) => s.classList.add('hidden'));

    btn.className =
      'w-full text-left px-3 py-2 rounded-lg font-bold text-white bg-studio-panel border border-studio-border flex items-center space-x-2.5 transition cursor-pointer';
    sec.classList.remove('hidden');
  }

  navProfiles.addEventListener('click', () => switchSection(navProfiles, secProfiles));
  navEmulators.addEventListener('click', () => {
    switchSection(navEmulators, secEmulators);
    loadEmulatorDetection();
  });
  navAssembler.addEventListener('click', () => switchSection(navAssembler, secAssembler));
  navEditor.addEventListener('click', () => switchSection(navEditor, secEditor));

  // Profiles list & editor rendering
  const profileListEl = container.querySelector('#profiles-list-container') as HTMLElement;
  const profileEditorEl = container.querySelector('#profile-editor-container') as HTMLElement;

  function renderProfileList() {
    profileListEl.innerHTML = Object.values(allProfiles)
      .map(
        (p) => `
      <div class="p-2.5 rounded-lg border transition cursor-pointer profile-item ${
        p.id === selectedProfileId
          ? 'bg-blue-600/20 border-blue-500 font-bold'
          : 'bg-studio-panel/60 border-studio-border hover:bg-studio-hover'
      }" data-id="${p.id}">
        <div class="flex items-center justify-between">
          <span class="text-white text-xs truncate">${p.name}</span>
          ${
            p.id === activeProfileId
              ? '<span class="px-1.5 py-0.2 bg-emerald-500/20 text-emerald-400 text-[9px] rounded-full font-bold">AKTIV</span>'
              : ''
          }
        </div>
        <div class="flex items-center space-x-2 text-[10px] text-studio-muted mt-1">
          <span>CPU: ${p.targetCpu}</span>
          <span>•</span>
          <span>${p.memory.ramLabel}</span>
        </div>
      </div>
    `
      )
      .join('');

    profileListEl.querySelectorAll('.profile-item').forEach((el) => {
      el.addEventListener('click', (e) => {
        const id = (e.currentTarget as HTMLElement).getAttribute('data-id');
        if (id) {
          selectedProfileId = id;
          renderProfileList();
          renderProfileEditor();
        }
      });
    });
  }

  function renderProfileEditor() {
    const p = allProfiles[selectedProfileId] || BUILTIN_PROFILES.amiga500;

    let emuDlButtonHtml = '';
    if (p.defaultEmulator === 'blastem') {
      emuDlButtonHtml = `
        <button id="btn-quick-dl-blastem" class="px-2.5 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded text-[10px] font-bold shrink-0 transition cursor-pointer flex items-center space-x-1">
          <span>⬇️ BlastEm Download</span>
        </button>
      `;
    } else if (p.defaultEmulator === 'hatari') {
      emuDlButtonHtml = `
        <button id="btn-quick-dl-hatari" class="px-2.5 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded text-[10px] font-bold shrink-0 transition cursor-pointer flex items-center space-x-1">
          <span>⬇️ Hatari Download</span>
        </button>
      `;
    } else if (p.defaultEmulator === 'fsuae') {
      emuDlButtonHtml = `
        <button id="btn-quick-dl-fsuae" class="px-2.5 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded text-[10px] font-bold shrink-0 transition cursor-pointer flex items-center space-x-1">
          <span>⬇️ FS-UAE Download</span>
        </button>
      `;
    } else if (p.defaultEmulator === 'winuae') {
      emuDlButtonHtml = `
        <button id="btn-quick-dl-winuae" class="px-2.5 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded text-[10px] font-bold shrink-0 transition cursor-pointer flex items-center space-x-1">
          <span>⬇️ WinUAE Download</span>
        </button>
      `;
    }

    let romDlButtonHtml = '';
    if (p.system === 'amiga') {
      romDlButtonHtml = `
        <button id="btn-quick-dl-aros" class="px-2.5 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded text-[10px] font-bold shrink-0 transition cursor-pointer flex items-center space-x-1">
          <span>⬇️ AROS Free ROM</span>
        </button>
      `;
    } else if (p.system === 'atarist') {
      romDlButtonHtml = `
        <button id="btn-quick-dl-emutos" class="px-2.5 py-1.5 bg-amber-600 hover:bg-amber-500 text-white rounded text-[10px] font-bold shrink-0 transition cursor-pointer flex items-center space-x-1">
          <span>⬇️ EmuTOS Free ROM</span>
        </button>
      `;
    }

    profileEditorEl.innerHTML = `
      <!-- Profile Header Card -->
      <div class="p-5 bg-studio-panel border border-studio-border rounded-xl space-y-4">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-base font-bold text-white">${p.name}</h3>
            <p class="text-studio-muted text-xs">ID: <span class="font-mono">${p.id}</span> ${
      p.isBuiltin ? '• (Integriertes Profil)' : '• (Benutzerdefiniert)'
    }</p>
          </div>
          <div class="flex items-center space-x-2">
            ${
              !p.isBuiltin
                ? `
              <button id="btn-delete-profile" class="px-3 py-1.5 bg-red-600/20 hover:bg-red-600/30 text-red-400 border border-red-500/30 rounded font-medium transition cursor-pointer">
                🗑️ Löschen
              </button>
            `
                : ''
            }
            <button id="btn-dup-profile" class="px-3 py-1.5 bg-studio-bg hover:bg-studio-hover border border-studio-border rounded text-white font-medium transition cursor-pointer">
              📋 Duplizieren
            </button>
            <button id="btn-activate-profile" class="px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded font-bold transition shadow cursor-pointer">
              ${p.id === activeProfileId ? '✅ Ist Aktiv' : '⚡ Als Aktives Profil setzen'}
            </button>
          </div>
        </div>
      </div>

      <!-- Settings Form -->
      <div class="grid grid-cols-2 gap-6">
        <!-- Col 1: Hardware & Capabilities -->
        <div class="space-y-4">
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
            <div class="font-bold text-white uppercase text-[11px] tracking-wider">1. Profil-Name & CPU</div>
            <div class="space-y-1.5">
              <label class="text-studio-muted font-semibold">Anzeigename:</label>
              <input type="text" id="prof-name" value="${p.name}" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-1.5 rounded outline-none focus:border-blue-500">
            </div>
            <div class="grid grid-cols-2 gap-3">
              <div class="space-y-1.5">
                <label class="text-studio-muted font-semibold">System-Typ:</label>
                <select id="prof-sys" class="w-full bg-studio-bg border border-studio-border text-white px-2.5 py-1.5 rounded outline-none focus:border-blue-500">
                  <option value="amiga" ${p.system === 'amiga' ? 'selected' : ''}>🕹️ Amiga</option>
                  <option value="megadrive" ${p.system === 'megadrive' ? 'selected' : ''}>🎮 Mega Drive</option>
                  <option value="atarist" ${p.system === 'atarist' ? 'selected' : ''}>🖥️ Atari ST</option>
                  <option value="baremetal" ${p.system === 'baremetal' ? 'selected' : ''}>⚡ Bare Metal</option>
                  <option value="custom" ${p.system === 'custom' ? 'selected' : ''}>⚙️ Custom</option>
                </select>
              </div>
              <div class="space-y-1.5">
                <label class="text-studio-muted font-semibold">Ziel-CPU:</label>
                <select id="prof-cpu" class="w-full bg-studio-bg border border-studio-border text-white px-2.5 py-1.5 rounded outline-none focus:border-blue-500">
                  <option value="68000" ${p.targetCpu === '68000' ? 'selected' : ''}>68000</option>
                  <option value="68010" ${p.targetCpu === '68010' ? 'selected' : ''}>68010</option>
                  <option value="68020" ${p.targetCpu === '68020' ? 'selected' : ''}>68020</option>
                  <option value="68030" ${p.targetCpu === '68030' ? 'selected' : ''}>68030</option>
                  <option value="68040" ${p.targetCpu === '68040' ? 'selected' : ''}>68040</option>
                  <option value="68060" ${p.targetCpu === '68060' ? 'selected' : ''}>68060</option>
                </select>
              </div>
            </div>
          </div>

          <!-- Studio Tools Capabilities Gating -->
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
            <div class="font-bold text-white uppercase text-[11px] tracking-wider">2. Sichtbare Studio-Werkzeuge (Tabs)</div>
            <div class="space-y-2">
              <label class="flex items-center space-x-2.5 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer">
                <input type="checkbox" id="cap-blitter" ${p.capabilities.hasBlitter ? 'checked' : ''} class="rounded text-blue-500">
                <div>
                  <div class="font-bold text-white">🧮 Amiga Blitter Studio</div>
                  <div class="text-[10px] text-studio-muted">Minterm-Kalkulator & Blitter-Register</div>
                </div>
              </label>
              <label class="flex items-center space-x-2.5 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer">
                <input type="checkbox" id="cap-copper" ${p.capabilities.hasCopper ? 'checked' : ''} class="rounded text-blue-500">
                <div>
                  <div class="font-bold text-white">🌈 Copperlist Visualizer</div>
                  <div class="text-[10px] text-studio-muted">Simulierter CRT-Rasterstrahl</div>
                </div>
              </label>
              <label class="flex items-center space-x-2.5 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer">
                <input type="checkbox" id="cap-bitplanes" ${p.capabilities.hasBitplanes ? 'checked' : ''} class="rounded text-blue-500">
                <div>
                  <div class="font-bold text-white">🎨 Planar Bitplane Studio</div>
                  <div class="text-[10px] text-studio-muted">RGB444 Farbquantisierer & Planar-Konvertierer</div>
                </div>
              </label>
              <label class="flex items-center space-x-2.5 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer">
                <input type="checkbox" id="cap-floppy" ${p.capabilities.hasAdfFloppy ? 'checked' : ''} class="rounded text-blue-500">
                <div>
                  <div class="font-bold text-white">💾 Virtual Floppy (ADF) Manager</div>
                  <div class="text-[10px] text-studio-muted">OFS/FFS Disketten-Inspektor in der Seitenleiste</div>
                </div>
              </label>
            </div>
          </div>
        </div>

        <!-- Col 2: Memory & Emulator -->
        <div class="space-y-4">
          <!-- Memory Map Configuration -->
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
            <div class="font-bold text-white uppercase text-[11px] tracking-wider">3. Speicher-Architektur (Memory Map)</div>
            <div class="grid grid-cols-2 gap-3">
              <div class="space-y-1.5">
                <label class="text-studio-muted font-semibold">RAM Größe (KB):</label>
                <input type="number" id="prof-ram-kb" value="${p.memory.ramTotalKb}" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-xs focus:border-blue-500">
              </div>
              <div class="space-y-1.5">
                <label class="text-studio-muted font-semibold">RAM Bezeichnung:</label>
                <input type="text" id="prof-ram-label" value="${p.memory.ramLabel}" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-1.5 rounded outline-none text-xs focus:border-blue-500">
              </div>
            </div>
          <!-- Assembler, Compiler & Build Configuration directly in Profile -->
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
            <div class="font-bold text-white uppercase text-[11px] tracking-wider">3. Assembler & Build Optionen</div>
            <div class="grid grid-cols-2 gap-3">
              <div class="space-y-1.5">
                <label class="text-studio-muted font-semibold">Standard Origin (ORG):</label>
                <input type="text" id="prof-origin" value="${p.buildOptions?.defaultOrigin || p.defaultOrigin}" class="w-full bg-studio-bg border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-xs focus:border-blue-500">
              </div>
              <div class="space-y-1.5">
                <label class="text-studio-muted font-semibold">Standard Ausgabeformat:</label>
                <select id="prof-output-fmt" class="w-full bg-studio-bg border border-studio-border text-white px-2.5 py-1.5 rounded outline-none focus:border-blue-500 text-xs cursor-pointer">
                  <option value="adf" ${(p.buildOptions?.defaultOutputFormat || p.defaultOutputFormat) === 'adf' ? 'selected' : ''}>Amiga Floppy Disk (.adf)</option>
                  <option value="bin" ${(p.buildOptions?.defaultOutputFormat || p.defaultOutputFormat) === 'bin' ? 'selected' : ''}>Raw Binary (.bin / ROM)</option>
                  <option value="srec" ${(p.buildOptions?.defaultOutputFormat || p.defaultOutputFormat) === 'srec' ? 'selected' : ''}>Motorola S-Record (.srec)</option>
                  <option value="hex" ${(p.buildOptions?.defaultOutputFormat || p.defaultOutputFormat) === 'hex' ? 'selected' : ''}>Intel Hex (.hex)</option>
                  <option value="elf" ${(p.buildOptions?.defaultOutputFormat || p.defaultOutputFormat) === 'elf' ? 'selected' : ''}>ELF32 Executable (.elf)</option>
                </select>
              </div>
            </div>
            <div class="grid grid-cols-2 gap-3 pt-1">
              <label class="flex items-center space-x-2 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer">
                <input type="checkbox" id="prof-opt-branch" ${p.buildOptions?.branchRelaxation !== 'strict' ? 'checked' : ''} class="rounded text-blue-500">
                <div>
                  <div class="font-bold text-white text-[11px]">Branch-Relaxation</div>
                  <div class="text-[9px] text-studio-muted">Auto Short / Word / Long</div>
                </div>
              </label>
              <label class="flex items-center space-x-2 p-2 bg-studio-bg border border-studio-border rounded cursor-pointer">
                <input type="checkbox" id="prof-opt-mapfile" ${p.buildOptions?.generateMapFile ? 'checked' : ''} class="rounded text-blue-500">
                <div>
                  <div class="font-bold text-white text-[11px]">Symbol-Mapfile (.map)</div>
                  <div class="text-[9px] text-studio-muted">Symboltabelle exportieren</div>
                </div>
              </label>
            </div>
          </div>

          <!-- Emulator Assignment for this Profile -->
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl space-y-3">
            <div class="font-bold text-white uppercase text-[11px] tracking-wider">4. Zugeordneter Emulator für dieses Profil</div>
            <div class="space-y-1.5">
              <label class="text-studio-muted font-semibold">Emulator Typ:</label>
              <select id="prof-emu" class="w-full bg-studio-bg border border-studio-border text-white px-2.5 py-1.5 rounded outline-none focus:border-blue-500">
                <option value="fsuae" ${p.defaultEmulator === 'fsuae' ? 'selected' : ''}>FS-UAE (Amiga)</option>
                <option value="winuae" ${p.defaultEmulator === 'winuae' ? 'selected' : ''}>WinUAE (Windows Amiga)</option>
                <option value="blastem" ${p.defaultEmulator === 'blastem' ? 'selected' : ''}>BlastEm (Mega Drive)</option>
                <option value="hatari" ${p.defaultEmulator === 'hatari' ? 'selected' : ''}>Hatari (Atari ST)</option>
                <option value="custom" ${p.defaultEmulator === 'custom' ? 'selected' : ''}>Custom / Anderer Emulator</option>
              </select>
            </div>
            <div class="space-y-1.5">
              <label class="text-studio-muted font-semibold">Eigener Executable-Pfad:</label>
              <div class="flex items-center space-x-2">
                <input type="text" id="prof-emu-path" value="${p.customExecutable || ''}" placeholder="z.B. C:\\WinUAE\\winuae64.exe" class="flex-1 bg-studio-bg border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-[11px] focus:border-blue-500">
                ${emuDlButtonHtml}
              </div>
            </div>
            <div class="space-y-1.5">
              <label class="text-studio-muted font-semibold">Kickstart / ROM Pfad (Optional):</label>
              <div class="flex items-center space-x-2">
                <input type="text" id="prof-rom-path" value="${p.customRomPath || ''}" placeholder="z.B. /home/user/kick31.rom" class="flex-1 bg-studio-bg border border-studio-border text-white px-3 py-1.5 rounded outline-none font-mono text-[11px] focus:border-blue-500">
                ${romDlButtonHtml}
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Save Profile Bar -->
      <div class="pt-4 flex items-center justify-end space-x-3 border-t border-studio-border">
        <button id="btn-save-profile" class="px-6 py-2 bg-blue-600 hover:bg-blue-500 text-white font-bold rounded-lg shadow transition cursor-pointer">
          💾 Profil Speichern
        </button>
      </div>
    `;

    // Quick download buttons wiring
    async function wireQuickDownload(btnId: string, emuId: string, isRom: boolean) {
      const btn = profileEditorEl.querySelector(`#${btnId}`) as HTMLElement | null;
      if (!btn) return;
      btn.addEventListener('click', async () => {
        btn.textContent = '⏳ Lädt...';
        const res = await api.installEmulator(emuId);
        if (res.success && res.executable_path) {
          btn.textContent = '✅ Installiert';
          if (isRom) {
            const romInput = profileEditorEl.querySelector('#prof-rom-path') as HTMLInputElement;
            if (romInput) romInput.value = res.executable_path;
            p.customRomPath = res.executable_path;
          } else {
            const pathInput = profileEditorEl.querySelector('#prof-emu-path') as HTMLInputElement;
            if (pathInput) pathInput.value = res.executable_path;
            p.customExecutable = res.executable_path;
          }
          allProfiles[p.id] = p;
          saveAllProfiles(allProfiles);
          showToast(res.message || 'Erfolgreich installiert!', 'success');
        } else {
          showToast(res.message || 'Download fehlgeschlagen.', 'error');
          btn.textContent = '❌ Fehler';
        }
      });
    }

    wireQuickDownload('btn-quick-dl-blastem', 'blastem', false);
    wireQuickDownload('btn-quick-dl-hatari', 'hatari', false);
    wireQuickDownload('btn-quick-dl-fsuae', 'fsuae', false);
    wireQuickDownload('btn-quick-dl-winuae', 'winuae', false);
    wireQuickDownload('btn-quick-dl-aros', 'aros_rom', true);
    wireQuickDownload('btn-quick-dl-emutos', 'emutos_rom', true);

    // Event listeners inside profile editor
    profileEditorEl.querySelector('#btn-save-profile')?.addEventListener('click', () => {
      const nameInput = profileEditorEl.querySelector('#prof-name') as HTMLInputElement;
      const sysSelect = profileEditorEl.querySelector('#prof-sys') as HTMLSelectElement;
      const cpuSelect = profileEditorEl.querySelector('#prof-cpu') as HTMLSelectElement;

      const capBlitter = profileEditorEl.querySelector('#cap-blitter') as HTMLInputElement;
      const capCopper = profileEditorEl.querySelector('#cap-copper') as HTMLInputElement;
      const capBitplanes = profileEditorEl.querySelector('#cap-bitplanes') as HTMLInputElement;
      const capFloppy = profileEditorEl.querySelector('#cap-floppy') as HTMLInputElement;

      const ramKbInput = profileEditorEl.querySelector('#prof-ram-kb') as HTMLInputElement;
      const ramLabelInput = profileEditorEl.querySelector('#prof-ram-label') as HTMLInputElement;
      const originInput = profileEditorEl.querySelector('#prof-origin') as HTMLInputElement;
      const outFmtSelect = profileEditorEl.querySelector('#prof-output-fmt') as HTMLSelectElement;
      const optBranch = profileEditorEl.querySelector('#prof-opt-branch') as HTMLInputElement;
      const optMapfile = profileEditorEl.querySelector('#prof-opt-mapfile') as HTMLInputElement;

      const emuSelect = profileEditorEl.querySelector('#prof-emu') as HTMLSelectElement;
      const emuPathInput = profileEditorEl.querySelector('#prof-emu-path') as HTMLInputElement;
      const romPathInput = profileEditorEl.querySelector('#prof-rom-path') as HTMLInputElement;

      const updated: PlatformProfile = {
        ...p,
        name: nameInput.value.trim() || p.name,
        system: sysSelect.value as any,
        targetCpu: cpuSelect.value,
        defaultOrigin: originInput.value.trim() || '$0000',
        defaultOutputFormat: outFmtSelect.value,
        capabilities: {
          hasBlitter: capBlitter.checked,
          hasCopper: capCopper.checked,
          hasBitplanes: capBitplanes.checked,
          hasAdfFloppy: capFloppy.checked,
          hasVdpTiles: sysSelect.value === 'megadrive',
        },
        memory: {
          ramTotalKb: parseInt(ramKbInput.value, 10) || 512,
          ramLabel: ramLabelInput.value.trim() || `${ramKbInput.value} KB RAM`,
          romTotalKb: p.memory.romTotalKb,
          romLabel: p.memory.romLabel,
        },
        buildOptions: {
          defaultOrigin: originInput.value.trim() || '$0000',
          defaultOutputFormat: outFmtSelect.value,
          branchRelaxation: optBranch?.checked ? 'auto' : 'strict',
          optimizeImmediates: true,
          generateMapFile: optMapfile?.checked || false,
        },
        defaultEmulator: emuSelect.value,
        customExecutable: emuPathInput.value.trim() || undefined,
        customRomPath: romPathInput.value.trim() || undefined,
      };

      allProfiles[p.id] = updated;
      saveAllProfiles(allProfiles);

      if (p.id === activeProfileId) {
        onProfileActivated(updated);
      }

      showToast(`Profil "${updated.name}" gespeichert!`, 'success');
      renderProfileList();
      renderProfileEditor();
    });

    profileEditorEl.querySelector('#btn-activate-profile')?.addEventListener('click', () => {
      activeProfileId = p.id;
      onProfileActivated(p);
      showToast(`Aktives Profil gesetzt: ${p.name}`, 'info');
      renderProfileList();
      renderProfileEditor();
    });

    profileEditorEl.querySelector('#btn-dup-profile')?.addEventListener('click', () => {
      const newId = `custom_${Date.now()}`;
      const cloned: PlatformProfile = {
        ...p,
        id: newId,
        name: `${p.name} (Kopie)`,
        isBuiltin: false,
      };
      allProfiles[newId] = cloned;
      saveAllProfiles(allProfiles);
      selectedProfileId = newId;
      showToast(`Profil dupliziert als "${cloned.name}"`, 'success');
      renderProfileList();
      renderProfileEditor();
    });

    profileEditorEl.querySelector('#btn-delete-profile')?.addEventListener('click', () => {
      showConfirmDialog({
        title: 'Profil löschen',
        message: `Möchtest du das benutzerdefinierte Profil "${p.name}" wirklich löschen?`,
        confirmText: 'Löschen',
        isDanger: true,
        onConfirm: () => {
          delete allProfiles[p.id];
          saveAllProfiles(allProfiles);
          selectedProfileId = 'amiga500';
          if (activeProfileId === p.id) {
            activeProfileId = 'amiga500';
            onProfileActivated(allProfiles.amiga500);
          }
          showToast(`Profil "${p.name}" gelöscht.`, 'info');
          renderProfileList();
          renderProfileEditor();
        },
      });
    });
  }

  // Add new custom profile
  container.querySelector('#btn-add-profile')?.addEventListener('click', () => {
    showPromptDialog({
      title: 'Neues Ziel-Profil anlegen',
      message: 'Gib einen Namen für das neue m68k Ziel-Profil ein:',
      placeholder: 'z.B. Amiga 1200 FastRAM oder Neo Geo MVS',
      confirmText: 'Erstellen',
      onConfirm: (name) => {
        const newId = `custom_${Date.now()}`;
        const newProf: PlatformProfile = {
          id: newId,
          name: name.trim(),
          system: 'custom',
          targetCpu: '68000',
          defaultOrigin: '$0000',
          defaultOutputFormat: 'bin',
          defaultEmulator: 'custom',
          capabilities: {
            hasCopper: false,
            hasBlitter: false,
            hasBitplanes: true,
            hasAdfFloppy: false,
            hasVdpTiles: false,
          },
          memory: {
            ramTotalKb: 1024,
            ramLabel: '1024 KB RAM',
            romTotalKb: 512,
            romLabel: '512 KB ROM',
          },
          buildOptions: {
            defaultOrigin: '$0000',
            defaultOutputFormat: 'bin',
            branchRelaxation: 'auto',
            optimizeImmediates: true,
            generateMapFile: false,
          },
          isBuiltin: false,
        };
        allProfiles[newId] = newProf;
        saveAllProfiles(allProfiles);
        selectedProfileId = newId;
        showToast(`Profil "${name}" erfolgreich angelegt!`, 'success');
        renderProfileList();
        renderProfileEditor();
      },
    });
  });

  // Emulator Detection & 1-Click Download View Logic
  async function loadEmulatorDetection() {
    const emuListEl = container.querySelector('#emu-list-container');
    if (!emuListEl) return;

    emuListEl.innerHTML = '<div class="text-studio-muted">Suche nach installierten Emulatoren im System...</div>';
    const detected: EmulatorProfile[] = await api.detectEmulators();

    emuListEl.innerHTML = `
      <div class="space-y-4">
        ${detected
          .filter((e) => e.id !== 'custom')
          .map(
            (e) => `
          <div class="p-4 bg-studio-panel border border-studio-border rounded-xl flex items-center justify-between">
            <div>
              <div class="flex items-center space-x-2 font-bold text-white text-xs">
                <span>${e.name}</span>
                <span class="font-mono text-[11px] text-studio-muted font-normal">(${e.executable_path})</span>
              </div>
              <div class="text-[10px] text-studio-muted mt-0.5">Unterstützte Formate: ${e.supported_formats.join(', ')}</div>
            </div>
            <div class="flex items-center space-x-3">
              ${
                e.available
                  ? '<span class="px-2.5 py-1 bg-emerald-500/20 text-emerald-400 font-bold rounded-lg text-xs">🟢 Verfügbar</span>'
                  : '<span class="px-2.5 py-1 bg-amber-500/20 text-amber-400 font-medium rounded-lg text-xs">🟡 Nicht gefunden</span>'
              }
              <button class="btn-install-emu px-3 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded text-xs font-bold transition shadow cursor-pointer" data-emuid="${e.id}">
                ${e.available ? '🔄 Aktualisieren' : '⬇️ 1-Klick Download'}
              </button>
            </div>
          </div>
        `
          )
          .join('')}

        <!-- Custom Emulator Info Card -->
        <div class="p-4 bg-studio-panel/50 border border-dashed border-studio-border rounded-xl flex items-center justify-between text-studio-muted">
          <div>
            <div class="font-bold text-xs text-white">⚙️ Eigener / Custom Emulator</div>
            <div class="text-[10px] mt-0.5">Pfad und Startparameter für eigene Emulatoren werden direkt im jeweiligen Profil unter 🎯 Profile verwalten eingetragen.</div>
          </div>
        </div>

        <!-- Open-Source Free ROMs -->
        <div class="pt-2 border-t border-studio-border">
          <div class="font-bold text-white uppercase text-[11px] tracking-wider mb-3">Freie Open-Source Kickstart & TOS ROMs</div>
          <div class="space-y-3">
            <div class="p-4 bg-studio-panel border border-studio-border rounded-xl flex items-center justify-between">
              <div>
                <div class="flex items-center space-x-2 font-bold text-white text-xs">
                  <span>🕹️ Amiga AROS 68k Kickstart Replacement ROM (Open-Source & Frei)</span>
                </div>
                <div class="text-[10px] text-studio-muted mt-0.5">Ermöglicht sofortige Amiga-Emulation in FS-UAE/WinUAE ohne kommerzielle Kickstart-Lizenz.</div>
              </div>
              <div>
                <button class="btn-install-emu px-3 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded text-xs font-bold transition shadow cursor-pointer" data-emuid="aros_rom">
                  ⬇️ 1-Klick AROS Download
                </button>
              </div>
            </div>

            <div class="p-4 bg-studio-panel border border-studio-border rounded-xl flex items-center justify-between">
              <div>
                <div class="flex items-center space-x-2 font-bold text-white text-xs">
                  <span>🖥️ Atari ST EmuTOS Replacement ROM (Open-Source & Frei)</span>
                </div>
                <div class="text-[10px] text-studio-muted mt-0.5">Ermöglicht sofortige Atari ST Emulation in Hatari ohne kommerzielles TOS-ROM.</div>
              </div>
              <div>
                <button class="btn-install-emu px-3 py-1.5 bg-amber-600 hover:bg-amber-500 text-white rounded text-xs font-bold transition shadow cursor-pointer" data-emuid="emutos_rom">
                  ⬇️ 1-Klick EmuTOS Download
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    `;

    emuListEl.querySelectorAll('.btn-install-emu').forEach((btn) => {
      btn.addEventListener('click', async (e) => {
        const targetBtn = e.currentTarget as HTMLElement;
        const emuId = targetBtn.getAttribute('data-emuid');
        if (!emuId || emuId === 'custom') return;

        targetBtn.textContent = '⏳ Lädt...';
        try {
          const res = await api.installEmulator(emuId);
          if (res.success && res.executable_path) {
            targetBtn.textContent = '✅ Installiert';
            alert(res.message || 'Erfolgreich installiert!');
            loadEmulatorDetection();
            // Automatically update profiles that match this emulator or ROM
            Object.values(allProfiles).forEach((p) => {
              if (p.defaultEmulator === emuId && !p.customExecutable) {
                p.customExecutable = res.executable_path;
              }
              if (emuId === 'aros_rom' && p.system === 'amiga' && !p.customRomPath) {
                p.customRomPath = res.executable_path;
              }
              if (emuId === 'emutos_rom' && p.system === 'atarist' && !p.customRomPath) {
                p.customRomPath = res.executable_path;
              }
            });
            saveAllProfiles(allProfiles);
          } else {
            targetBtn.textContent = '❌ Fehler';
            alert(res.message || 'Download fehlgeschlagen.');
          }
        } catch (err: any) {
          targetBtn.textContent = '❌ Fehler';
          alert(`Download-Fehler: ${err.message || err}`);
        }
      });
    });
  }

  // General settings inputs
  const genOutputFmt = container.querySelector('#gen-output-fmt') as HTMLSelectElement;
  const genTheme = container.querySelector('#gen-theme') as HTMLSelectElement;
  const genFontSize = container.querySelector('#gen-font-size') as HTMLSelectElement;
  const genTabSize = container.querySelector('#gen-tab-size') as HTMLSelectElement;
  const genMinimap = container.querySelector('#gen-minimap') as HTMLInputElement;

  function syncGeneralSettings() {
    genOutputFmt.value = generalSettings.defaultOutputFormat;
    genTheme.value = generalSettings.theme;
    genFontSize.value = `${generalSettings.editorFontSize}`;
    genTabSize.value = `${generalSettings.editorTabSize}`;
    genMinimap.checked = generalSettings.minimapEnabled;
  }

  function saveAndApplyGeneral() {
    generalSettings = {
      defaultOrigin: generalSettings.defaultOrigin,
      defaultOutputFormat: genOutputFmt.value,
      theme: genTheme.value,
      editorFontSize: parseInt(genFontSize.value, 10) || 13,
      editorTabSize: parseInt(genTabSize.value, 10) || 4,
      minimapEnabled: genMinimap.checked,
    };
    saveGeneralSettings(generalSettings);
    onGeneralSettingsChanged(generalSettings);
  }

  [genOutputFmt, genTheme, genFontSize, genTabSize, genMinimap].forEach((el) => {
    el.addEventListener('change', saveAndApplyGeneral);
  });

  function refresh() {
    allProfiles = loadAllProfiles();
    generalSettings = loadGeneralSettings();
    syncGeneralSettings();
    renderProfileList();
    renderProfileEditor();
  }

  refresh();

  return { element: container, refresh };
}
