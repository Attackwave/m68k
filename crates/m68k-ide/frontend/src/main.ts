//! m68k Studio Main Application Entry Point.

import './styles/main.css';
import * as monaco from 'monaco-editor';
import { api, AssembleResponse } from './api';
import { registerM68kLanguage } from './editor/m68k-language';
import { createHeader } from './components/Header';
import { createSidebar } from './components/Sidebar';
import { createDisassemblerPanel } from './components/DisassemblerPanel';
import { createBitplaneStudio } from './components/BitplaneStudio';
import { createCopperVisualizer } from './components/CopperVisualizer';
import { createRegistersPanel } from './components/RegistersPanel';
import { createBottomConsole } from './components/BottomConsole';

const INITIAL_SOURCE = `; Amiga 500 Bare-Metal Copper Raster Demo
; Powered by m68k Studio IDE & m68k-lsp
    SECTION Code,CODE

    ; Amiga Hardware Registers
CUSTOM_BASE     EQU $DFF000
DMACON          EQU $096
INTENA          EQU $09A
COP1LCH         EQU $080
COPJMP1         EQU $088
COLOR00         EQU $180
BPLCON0         EQU $100

Start:
    ; 1. Take over hardware
    move.l  $4.w,a6             ; ExecBase
    suba.l  a1,a1
    jsr     -$126(a6)           ; FindTask(NULL)

    lea     CUSTOM_BASE,a6
    move.w  #$7FFF,DMACON(a6)   ; Disable DMA
    move.w  #$7FFF,INTENA(a6)   ; Disable Interrupts

    ; 2. Install Copperlist
    lea     CopperList(pc),a0
    move.l  a0,COP1LCH(a6)
    move.w  #0,COPJMP1(a6)
    move.w  #$8280,DMACON(a6)   ; Enable DMA + Copper

MainLoop:
    ; Check left mouse button (CIAA PRA bit 6)
    btst    #6,$BFE001
    bne.s   MainLoop

    ; 3. Restore system and exit
    move.w  #$8020,DMACON(a6)
    rts

    SECTION Data,DATA_C

CopperList:
    dc.w    BPLCON0,$0000       ; 0 bitplanes
    dc.w    COLOR00,$0002       ; Dark blue background
    dc.w    $8001,$FFFE         ; Wait line 128
    dc.w    COLOR00,$0F80       ; Orange raster bar
    dc.w    $8801,$FFFE         ; Wait line 136
    dc.w    COLOR00,$0002       ; Restore blue
    dc.w    $FFFF,$FFFE         ; End of copperlist
`;

async function initApp() {
  const appContainer = document.getElementById('app') as HTMLElement;

  // 1. Register m68k syntax and LSP hooks in Monaco
  registerM68kLanguage();

  let activeCpu = '68000';
  let currentBinary: number[] = [];

  // 2. Create UI Panels
  const consolePanel = createBottomConsole({
    onErrorClick: (line) => {
      editor.revealLineInCenter(line);
      editor.setPosition({ lineNumber: line, column: 1 });
      editor.focus();
    },
  });

  const disasmPanel = createDisassemblerPanel();
  const bitplaneStudio = createBitplaneStudio();
  const copperVisualizer = createCopperVisualizer();
  const registersPanel = createRegistersPanel();

  const sidebarPanel = createSidebar({
    files: [
      { name: 'main.s', path: 'src/main.s' },
      { name: 'custom.i', path: 'includes/custom.i' },
      { name: 'project.json', path: 'project.json' },
    ],
    activeFile: 'src/main.s',
    onFileSelect: (path) => consolePanel.log(`Switched file: ${path}`),
    onNewFile: () => consolePanel.log('Created new source file'),
    onInspectAdf: () => {},
    adfInfo: null,
  });

  const header = createHeader({
    projectName: 'Amiga500Demo',
    targetCpu: activeCpu,
    onCpuChange: (cpu) => {
      activeCpu = cpu;
      consolePanel.log(`Target CPU changed to ${cpu}`);
      updateDiagnostics();
    },
    onBuild: handleBuild,
    onRun: handleRun,
    onFormat: handleFormat,
    onNewProject: (template) => consolePanel.log(`Scaffolding template: ${template}`),
    onThemeToggle: () => {
      document.body.classList.toggle('theme-amiga');
    },
  });

  // 3. Assemble Layout Structure
  appContainer.appendChild(header);

  const mainArea = document.createElement('div');
  mainArea.className = 'flex-1 flex overflow-hidden';

  // Left sidebar
  mainArea.appendChild(sidebarPanel.element);

  // Center editor & tools container
  const centerContainer = document.createElement('div');
  centerContainer.className = 'flex-1 flex flex-col overflow-hidden bg-studio-bg';

  // Center Tabs Header
  const centerTabsHeader = document.createElement('div');
  centerTabsHeader.className = 'h-9 bg-studio-sidebar border-b border-studio-border flex items-center px-3 space-x-1 shrink-0';
  centerTabsHeader.innerHTML = `
    <button id="tab-center-editor" class="px-3 py-1 text-xs font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5">
      <span>📝 Code Editor</span>
    </button>
    <button id="tab-center-disasm" class="px-3 py-1 text-xs font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5">
      <span>🔍 Disassembly Trace</span>
    </button>
    <button id="tab-center-bitplane" class="px-3 py-1 text-xs font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5">
      <span>🎨 Bitplane Studio</span>
    </button>
    <button id="tab-center-copper" class="px-3 py-1 text-xs font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5">
      <span>🌈 Copper Visualizer</span>
    </button>
  `;
  centerContainer.appendChild(centerTabsHeader);

  // View Containers
  const editorHost = document.createElement('div');
  editorHost.className = 'flex-1 overflow-hidden relative';

  const disasmHost = disasmPanel.element;
  disasmHost.classList.add('hidden');

  const bitplaneHost = bitplaneStudio;
  bitplaneHost.classList.add('hidden');

  const copperHost = copperVisualizer.element;
  copperHost.classList.add('hidden');

  centerContainer.appendChild(editorHost);
  centerContainer.appendChild(disasmHost);
  centerContainer.appendChild(bitplaneHost);
  centerContainer.appendChild(copperHost);

  // Add Bottom Console to Center
  centerContainer.appendChild(consolePanel.element);

  mainArea.appendChild(centerContainer);

  // Right sidebar
  mainArea.appendChild(registersPanel);

  appContainer.appendChild(mainArea);

  // 4. Initialize Monaco Editor Instance
  const editor = monaco.editor.create(editorHost, {
    value: INITIAL_SOURCE,
    language: 'm68k',
    theme: 'vs-dark',
    automaticLayout: true,
    fontSize: 13,
    fontFamily: '"JetBrains Mono", "Fira Code", Consolas, monospace',
    minimap: { enabled: true },
    scrollBeyondLastLine: false,
    lineNumbers: 'on',
    renderLineHighlight: 'all',
    tabSize: 4,
    bracketPairColorization: { enabled: true },
  });

  // Center Tab Switching
  const tabEditor = centerTabsHeader.querySelector('#tab-center-editor') as HTMLElement;
  const tabDisasm = centerTabsHeader.querySelector('#tab-center-disasm') as HTMLElement;
  const tabBitplane = centerTabsHeader.querySelector('#tab-center-bitplane') as HTMLElement;
  const tabCopper = centerTabsHeader.querySelector('#tab-center-copper') as HTMLElement;

  function switchCenterTab(tabName: string) {
    [tabEditor, tabDisasm, tabBitplane, tabCopper].forEach((t) => {
      t.className = 'px-3 py-1 text-xs font-medium text-studio-muted hover:text-white transition flex items-center space-x-1.5';
    });
    [editorHost, disasmHost, bitplaneHost, copperHost].forEach((h) => h.classList.add('hidden'));

    if (tabName === 'editor') {
      tabEditor.className = 'px-3 py-1 text-xs font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5';
      editorHost.classList.remove('hidden');
      editor.layout();
    } else if (tabName === 'disasm') {
      tabDisasm.className = 'px-3 py-1 text-xs font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5';
      disasmHost.classList.remove('hidden');
    } else if (tabName === 'bitplane') {
      tabBitplane.className = 'px-3 py-1 text-xs font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5';
      bitplaneHost.classList.remove('hidden');
    } else if (tabName === 'copper') {
      tabCopper.className = 'px-3 py-1 text-xs font-bold text-white bg-studio-panel border border-studio-border rounded flex items-center space-x-1.5';
      copperHost.classList.remove('hidden');
    }
  }

  tabEditor.addEventListener('click', () => switchCenterTab('editor'));
  tabDisasm.addEventListener('click', () => switchCenterTab('disasm'));
  tabBitplane.addEventListener('click', () => switchCenterTab('bitplane'));
  tabCopper.addEventListener('click', () => switchCenterTab('copper'));

  // 5. Diagnostics & Linting Updates
  async function updateDiagnostics() {
    const source = editor.getValue();
    const diags = await api.lspDiagnostics(source, activeCpu);

    const markers: monaco.editor.IMarkerData[] = diags.map((d) => ({
      severity: d.severity === 1 ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
      message: d.message,
      startLineNumber: d.start_line + 1,
      startColumn: d.start_col + 1,
      endLineNumber: d.end_line + 1,
      endColumn: d.end_col + 1,
    }));

    const model = editor.getModel();
    if (model) {
      monaco.editor.setModelMarkers(model, 'm68k-linter', markers);
    }
  }

  let diagTimer: any = null;
  editor.onDidChangeModelContent(() => {
    clearTimeout(diagTimer);
    diagTimer = setTimeout(updateDiagnostics, 300);
  });

  // 6. Build Handler
  async function handleBuild() {
    consolePanel.log(`Assembling with target CPU ${activeCpu}...`);
    const source = editor.getValue();
    const res: AssembleResponse = await api.assemble({ source, cpu: activeCpu });

    consolePanel.setErrors(res.errors, res.warnings);

    if (res.success && res.binary_base64) {
      const binaryString = atob(res.binary_base64);
      currentBinary = Array.from(binaryString, (c) => c.charCodeAt(0));

      consolePanel.log(`Assembly successful! Generated ${res.byte_count} bytes.`);

      // Update Disassembly View
      const disasmRes = await api.disassemble(currentBinary, 0, activeCpu);
      disasmPanel.updateLines(disasmRes.lines);

      // Update Copper Visualizer
      const copperRes = await api.parseCopper(currentBinary);
      copperVisualizer.updateInstructions(copperRes.instructions);

      // Update Virtual ADF Floppy
      const adfBytes = await api.createAdf('DemoDisk', false, currentBinary);
      const adfInfo = await api.inspectAdf(adfBytes);
      sidebarPanel.updateAdf(adfInfo);
    }
  }

  // 7. Run Handler
  async function handleRun() {
    await handleBuild();
    consolePanel.log('Launching configured emulator...');
    try {
      const resp = await api.launchEmulator('fsuae', 'target/demo.adf');
      consolePanel.log(resp.message);
    } catch (e: any) {
      consolePanel.log(`Emulator launch notice: ${e.message || e}`, false);
    }
  }

  // 8. Format Handler
  async function handleFormat() {
    const source = editor.getValue();
    const formatted = await api.lspFormat(source);
    if (formatted !== source) {
      editor.setValue(formatted);
      consolePanel.log('Formatted assembly source.');
    }
  }

  // 9. Global Keyboard Shortcuts
  window.addEventListener('keydown', (e) => {
    if (e.key === 'F7') {
      e.preventDefault();
      handleBuild();
    } else if (e.key === 'F5') {
      e.preventDefault();
      handleRun();
    } else if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'i') {
      e.preventDefault();
      handleFormat();
    } else if (e.ctrlKey && e.key.toLowerCase() === 's') {
      e.preventDefault();
      consolePanel.log('Source saved (Ctrl+S).');
    }
  });

  // Initial diagnostics pass
  updateDiagnostics();
}

// Start app
window.addEventListener('DOMContentLoaded', initApp);
