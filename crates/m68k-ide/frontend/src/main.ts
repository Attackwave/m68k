//! m68k Studio Main Frontend Entrypoint
//! Unified VS Code Studio Architecture with First-Class Retro Tool Tabs & Live ASM Preview.

import './styles/main.css';
import * as monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import {
  api,
  AssembleResponse,
  IdeDiagnosticItem,
} from './api';
import { StudioProject } from './project';
import {
  workspace,
  OpenProject,
  lastProjectRoot,
  rememberLastProject,
  forgetLastProject,
} from './workspace';
import { createStartScreen } from './components/StartScreen';
import {
  PlatformProfile,
  loadAllProfiles,
  getActiveProfile,
} from './profiles';
import { registerM68kLanguage } from './editor/m68k-language';
import { registerM68kPythonLanguage } from './editor/python-language';

// Components
import { createTitleBar } from './components/TitleBar';
import { createActivityBar, ActivityViewId } from './components/ActivityBar';
import { createProjectExplorer } from './components/ProjectExplorer';
import { createSidebar } from './components/Sidebar';
import { createEditorTabs } from './components/EditorTabs';
import { createSearchView } from './components/SearchView';
import { createRunDebugView } from './components/RunDebugView';
import { createRetroStudioView, RetroToolId } from './components/RetroStudioView';
import { createBottomPanel } from './components/BottomPanel';
import { createStatusBar } from './components/StatusBar';
import { showCommandPalette, CommandItem } from './components/CommandPalette';
import { showContextMenu } from './components/ContextMenu';
import { showToast } from './components/NotificationSystem';

// Studio Full Views
import { createBlitterStudio } from './components/BlitterStudio';
import { createBitplaneStudio } from './components/BitplaneStudio';
import { createCopperVisualizer } from './components/CopperVisualizer';
import { createMemoryMapPanel } from './components/MemoryMapPanel';
import { createDisassemblerPanel } from './components/DisassemblerPanel';
import { createHexMemoryViewer } from './components/HexMemoryViewer';
import { createHardwareRegistersPanel } from './components/HardwareRegistersPanel';
import {
  createSettingsView,
  AppGeneralSettings,
  loadGeneralSettings,
  saveGeneralSettings,
} from './components/SettingsView';

// Setup Monaco web worker
(self as any).MonacoEnvironment = {
  getWorker() {
    return new editorWorker();
  },
};

/// Read an opened project's source files into the shell's project shape.
///
/// Only text sources are pulled in; `build/` is already excluded by the
/// backend's tree walk, and binary artifacts have no place in an editor
/// buffer.
async function loadProjectFromDisk(opened: OpenProject): Promise<StudioProject> {
  const files: Record<string, string> = {};
  const sources = opened.files.filter((f) => !f.is_dir && isTextFile(f.path));

  await Promise.all(
    sources.map(async (f) => {
      try {
        files[f.path] = await workspace.readFile(opened.root, f.path);
      } catch {
        // A file that vanished between listing and reading should not
        // stop the project from opening.
      }
    })
  );

  return {
    id: opened.root,
    name: opened.manifest.name,
    version: opened.manifest.version,
    profileId: opened.manifest.profile,
    mainFile: opened.manifest.build.main,
    files,
    created: Date.now(),
    lastModified: Date.now(),
  };
}

const TEXT_EXTENSIONS = ['.s', '.asm', '.i', '.inc', '.py', '.md', '.json', '.txt', '.cfg'];

function isTextFile(path: string): boolean {
  const lower = path.toLowerCase();
  return TEXT_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

async function initApp(opened: OpenProject) {
  const appContainer = document.getElementById('app') as HTMLElement;
  appContainer.innerHTML = '';

  // 1. Register m68k syntax, Python support and LSP hooks in Monaco
  registerM68kLanguage();
  registerM68kPythonLanguage();

  let generalSettings: AppGeneralSettings = loadGeneralSettings();
  let allProfiles = loadAllProfiles();

  // The project lives on disk. Its files are read through the workspace
  // API rather than held in localStorage, so an external editor, a build
  // and version control all see the same bytes.
  const projectRoot = opened.root;
  let currentProject: StudioProject = await loadProjectFromDisk(opened);

  /// Persist the project to disk.
  ///
  /// Callers mutate `currentProject.files` and then call this, so it
  /// writes back every file whose content differs from what was last
  /// read — one place, rather than a write at each of the ten call
  /// sites that used to reach localStorage.
  const savedContents = new Map<string, string>(Object.entries(currentProject.files));

  function saveCurrentProject(project: StudioProject) {
    for (const [path, contents] of Object.entries(project.files)) {
      if (savedContents.get(path) === contents) continue;
      savedContents.set(path, contents);
      void workspace.writeFile(projectRoot, path, contents).catch((e) => {
        savedContents.delete(path);
        showToast(
          `${path} konnte nicht gespeichert werden: ${e instanceof Error ? e.message : e}`,
          'error'
        );
      });
    }
  }

  let currentProfile: PlatformProfile = getActiveProfile(currentProject.profileId);
  let activeCpu = currentProfile.targetCpu;

  // Multi-File open tabs state (supports both files and 'tool:blitter' virtual tabs)
  const openFiles: string[] = Object.keys(currentProject.files).slice(0, 4);
  let activeFile = currentProject.mainFile || openFiles[0] || 'src/main.py';
  if (!openFiles.includes(activeFile)) {
    openFiles.unshift(activeFile);
  }
  const dirtyFiles = new Set<string>();
  let isAsmPreviewActive = false;

  let currentBinary: number[] = [];
  let currentAdfBytes: number[] = [];

  // Sidebar and Panel states
  let isSidebarVisible = true;
  let isPanelVisible = true;
  let currentActivityView: ActivityViewId = 'explorer';

  // --- Initialize Subsystem Panels & Tools ---
  const disasmPanel = createDisassemblerPanel();
  const hexMemoryViewer = createHexMemoryViewer();
  const copperVisualizer = createCopperVisualizer();
  const memoryMapPanel = createMemoryMapPanel();
  const bitplaneStudio = createBitplaneStudio((code: string) => {
    const pos = editor.getPosition();
    if (pos) {
      editor.executeEdits('bitplane-studio', [
        {
          range: new monaco.Range(pos.lineNumber, pos.column, pos.lineNumber, pos.column),
          text: '\n' + code + '\n',
        },
      ]);
      bottomPanel.log('Inserted Bitplane/Copper assembly into current editor position.');
      switchToFile(activeFile.startsWith('tool:') ? currentProject.mainFile || 'src/main.py' : activeFile);
    }
  });
  const blitterStudio = createBlitterStudio((code: string) => {
    const pos = editor.getPosition();
    if (pos) {
      editor.executeEdits('blitter-studio', [
        {
          range: new monaco.Range(pos.lineNumber, pos.column, pos.lineNumber, pos.column),
          text: '\n' + code + '\n',
        },
      ]);
      bottomPanel.log('Inserted Blitter code into current editor position.');
      switchToFile(activeFile.startsWith('tool:') ? currentProject.mainFile || 'src/main.py' : activeFile);
    }
  });

  const hardwareRegistersPanel = createHardwareRegistersPanel((addressHex) => {
    navigator.clipboard.writeText(addressHex);
    bottomPanel.log(`Copied register address ${addressHex} to clipboard.`);
    showToast(`Adresse ${addressHex} kopiert`, 'info');
  });
  hardwareRegistersPanel.loadRegisters();

  const settingsView = createSettingsView(
    currentProfile.id,
    (updatedProfile) => {
      applyPlatformProfile(updatedProfile.id);
      allProfiles = loadAllProfiles();
      titleBar.updateProfilesList(allProfiles, currentProfile.id);
    },
    (updatedSettings) => {
      generalSettings = updatedSettings;
      saveGeneralSettings(generalSettings);
      applySettingsToEditor(generalSettings);
      bottomPanel.log('General editor settings updated.');
    }
  );

  // --- Project Explorer ---
  const projectExplorer = createProjectExplorer({
    project: currentProject,
    activeFile,
    onFileSelect: switchToFile,
    onFileCreate: (path, initialContent) => {
      currentProject.files[path] =
        initialContent !== undefined
          ? initialContent
          : path.endsWith('.py')
          ? '# New m68k Python Source\n'
          : '; New m68k Assembly Source\n';
      saveCurrentProject(currentProject);
      if (!openFiles.includes(path)) openFiles.push(path);
      switchToFile(path);
      projectExplorer.updateProject(currentProject, activeFile);
      editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
      searchView.updateFiles(currentProject.files);
      showToast(`Datei erstellt: ${path}`, 'success');
      updateDiagnostics();
    },
    onFileRename: (oldPath, newPath) => {
      if (currentProject.files[oldPath] !== undefined) {
        const content = currentProject.files[oldPath];
        delete currentProject.files[oldPath];
        currentProject.files[newPath] = content;
        if (currentProject.mainFile === oldPath) currentProject.mainFile = newPath;
        const idx = openFiles.indexOf(oldPath);
        if (idx !== -1) openFiles[idx] = newPath;
        if (activeFile === oldPath) activeFile = newPath;
        saveCurrentProject(currentProject);
        projectExplorer.updateProject(currentProject, activeFile);
        editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
        searchView.updateFiles(currentProject.files);
        showToast(`Datei umbenannt zu "${newPath}"`, 'success');
      }
    },
    onFileDelete: (path) => {
      delete currentProject.files[path];
      const idx = openFiles.indexOf(path);
      if (idx !== -1) openFiles.splice(idx, 1);
      const remaining = Object.keys(currentProject.files);
      if (activeFile === path) {
        activeFile = openFiles[0] || remaining[0] || 'src/main.py';
        switchToFile(activeFile);
      }
      if (currentProject.mainFile === path) {
        currentProject.mainFile = remaining[0] || 'src/main.py';
      }
      saveCurrentProject(currentProject);
      projectExplorer.updateProject(currentProject, activeFile);
      editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
      searchView.updateFiles(currentProject.files);
      showToast(`Datei "${path}" gelöscht.`, 'info');
      updateDiagnostics();
    },
    onSetMainFile: (path) => {
      currentProject.mainFile = path;
      saveCurrentProject(currentProject);
      projectExplorer.updateProject(currentProject, activeFile);
    },
    onNewProjectWizard: openNewProjectWizard,
    onExportProject: exportProjectAsJson,
  });

  // --- Explorer Sidebar (Files, Dynamic Target Artifacts, Symbols) ---
  const sidebarFilesContainer = createSidebar({
    explorerElement: projectExplorer.element,
    currentProfile,
    onDownloadArtifact: downloadTargetArtifact,
    adfInfo: null,
  });

  // --- Search View ---
  const searchView = createSearchView({
    files: currentProject.files,
    onNavigate: (file, line, col) => {
      switchToFile(file);
      editor.revealLineInCenter(line);
      editor.setPosition({ lineNumber: line, column: col });
      editor.focus();
    },
    onReplaceAll: (fileReplacements) => {
      for (const [fPath, reps] of fileReplacements.entries()) {
        let content = currentProject.files[fPath];
        if (!content) continue;
        for (const r of reps) {
          content = content.replaceAll(r.oldText, r.newText);
        }
        currentProject.files[fPath] = content;
        if (fPath === activeFile) {
          editor.setValue(content);
        }
      }
      saveCurrentProject(currentProject);
      searchView.updateFiles(currentProject.files);
      showToast('Alle Ersetzungen erfolgreich durchgeführt!', 'success');
      updateDiagnostics();
    },
  });

  // --- Retro Studio Sidebar View (Launches Tools into Editor Tabs) ---
  const retroStudioView = createRetroStudioView({
    onOpenTool: (toolId: RetroToolId) => {
      if (toolId === 'hwregs') {
        isPanelVisible = true;
        bottomPanel.element.classList.remove('hidden');
        bottomPanel.setTab('hwregs');
        showToast('Hardwareregister DB im unteren Panel geöffnet', 'info');
      } else {
        switchToFile(`tool:${toolId}`);
      }
    },
  });

  // --- Run & Debug Sidebar View ---
  const runDebugView = createRunDebugView({
    currentProfile,
    profiles: allProfiles,
    onProfileChange: applyPlatformProfile,
    onRun: handleRun,
    onBuild: handleBuild,
    onOpenSettings: () => switchToFile('tool:settings'),
  });

  // --- Editor Multi-File Tabs (with Tool Tab & Split ASM Preview support) ---
  const editorTabs = createEditorTabs({
    openFiles,
    activeFile,
    dirtyFiles,
    isAsmPreviewActive,
    onSelectTab: switchToFile,
    onCloseTab: (path) => {
      const idx = openFiles.indexOf(path);
      if (idx !== -1) {
        openFiles.splice(idx, 1);
        if (activeFile === path) {
          const nextFile =
            openFiles[Math.max(0, idx - 1)] || Object.keys(currentProject.files)[0] || 'src/main.py';
          if (nextFile) switchToFile(nextFile);
        }
        editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
      }
    },
    onNewFile: () => {
      projectExplorer.element.querySelector('#exp-btn-new-file')?.dispatchEvent(new MouseEvent('click'));
    },
    onToggleAsmPreview: () => {
      isAsmPreviewActive = !isAsmPreviewActive;
      editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
      updateAsmPreview();
    },
  });

  // --- Bottom Panel (Problems, Output, Disassembly, Hex Dump, Hardware Registers) ---
  const bottomPanel = createBottomPanel({
    initialTab: 'problems',
    onSelectProblem: (line, col) => {
      if (activeFile.startsWith('tool:')) {
        switchToFile(currentProject.mainFile || 'src/main.py');
      }
      showCenterView('editor');
      editor.revealLineInCenter(line);
      editor.setPosition({ lineNumber: line, column: col });
      editor.focus();
    },
    onClosePanel: () => {
      isPanelVisible = false;
      bottomPanel.element.classList.add('hidden');
    },
  });

  // Mount Views into Bottom Panel
  bottomPanel.attachCustomView('disasm', disasmPanel.element);
  bottomPanel.attachCustomView('hex', hexMemoryViewer.element);
  bottomPanel.attachCustomView('hwregs', hardwareRegistersPanel.element);

  // --- Status Bar ---
  const statusBar = createStatusBar({
    activeProfileName: currentProfile.name,
    activeCpu,
    errorCount: 0,
    warningCount: 0,
    cursorLine: 1,
    cursorCol: 1,
    tabSize: generalSettings.editorTabSize || 4,
    language: activeFile.endsWith('.py') ? 'python' : 'm68k',
    cycleInfo: '⚡ ~0 Cycles',
    onOpenProblems: () => {
      isPanelVisible = true;
      bottomPanel.element.classList.remove('hidden');
      bottomPanel.setTab('problems');
    },
    onOpenProfileSettings: () => switchToFile('tool:settings'),
  });

  // --- Activity Bar ---
  const activityBar = createActivityBar({
    activeView: currentActivityView,
    sidebarVisible: isSidebarVisible,
    onViewChange: (viewId) => {
      currentActivityView = viewId;
      isSidebarVisible = true;
      updateSidebarView();
    },
    onToggleSidebar: () => {
      isSidebarVisible = !isSidebarVisible;
      updateSidebarVisibility();
    },
    onOpenSettings: () => switchToFile('tool:settings'),
    onOpenTemplates: openNewProjectWizard,
  });

  // --- Title Bar ---
  const titleBar = createTitleBar({
    projectName: currentProject.name,
    activeProfileId: currentProfile.id,
    profiles: allProfiles,
    onProfileChange: applyPlatformProfile,
    onBuild: handleBuild,
    onRun: handleRun,
    onFormat: handleFormat,
    onOpenCommandPalette: (mode) => openCommandPalette(mode),
    onToggleSidebar: () => {
      isSidebarVisible = !isSidebarVisible;
      updateSidebarVisibility();
    },
    onTogglePanel: () => {
      isPanelVisible = !isPanelVisible;
      bottomPanel.element.classList.toggle('hidden', !isPanelVisible);
    },
    onOpenSettings: () => switchToFile('tool:settings'),
    onNewProject: openNewProjectWizard,
    onNewFile: () => {
      projectExplorer.element.querySelector('#exp-btn-new-file')?.dispatchEvent(new MouseEvent('click'));
    },
    onExportProject: exportProjectAsJson,
    onThemeToggle: () => document.body.classList.toggle('theme-amiga'),
  });

  // --- Layout Assembly ---
  appContainer.appendChild(titleBar.element);

  const workbenchArea = document.createElement('div');
  workbenchArea.className = 'flex-1 flex overflow-hidden relative';

  // Left Activity Bar
  workbenchArea.appendChild(activityBar.element);

  // Primary Sidebar Container
  const primarySidebarContainer = document.createElement('div');
  primarySidebarContainer.id = 'primary-sidebar-container';
  primarySidebarContainer.className =
    'w-64 bg-studio-sidebar border-r border-studio-border flex flex-col shrink-0 overflow-hidden';

  const viewHolderExplorer = document.createElement('div');
  viewHolderExplorer.className = 'h-full flex flex-col';
  viewHolderExplorer.appendChild(sidebarFilesContainer.element);

  const viewHolderSearch = document.createElement('div');
  viewHolderSearch.className = 'h-full flex flex-col hidden';
  viewHolderSearch.appendChild(searchView.element);

  const viewHolderRetro = document.createElement('div');
  viewHolderRetro.className = 'h-full flex flex-col hidden';
  viewHolderRetro.appendChild(retroStudioView.element);

  const viewHolderDebug = document.createElement('div');
  viewHolderDebug.className = 'h-full flex flex-col hidden';
  viewHolderDebug.appendChild(runDebugView.element);

  primarySidebarContainer.appendChild(viewHolderExplorer);
  primarySidebarContainer.appendChild(viewHolderSearch);
  primarySidebarContainer.appendChild(viewHolderRetro);
  primarySidebarContainer.appendChild(viewHolderDebug);

  workbenchArea.appendChild(primarySidebarContainer);

  // Center Editor & Bottom Panel Container
  const centerWorkbench = document.createElement('div');
  centerWorkbench.className = 'flex-1 flex flex-col overflow-hidden bg-studio-bg';

  // Top Editor Tabs & Breadcrumbs
  centerWorkbench.appendChild(editorTabs.element);

  // Center Views Wrapper (Monaco Split View & Tools)
  const centerViewsWrapper = document.createElement('div');
  centerViewsWrapper.className = 'flex-1 relative overflow-hidden';

  // Monaco Split Editor Container
  const editorWrapper = document.createElement('div');
  editorWrapper.id = 'monaco-editor-wrapper';
  editorWrapper.className = 'absolute inset-0 flex flex-row overflow-hidden';

  const editorContainer = document.createElement('div');
  editorContainer.id = 'monaco-editor-container';
  editorContainer.className = 'flex-1 h-full relative';

  const asmPreviewContainer = document.createElement('div');
  asmPreviewContainer.id = 'monaco-asm-preview-container';
  asmPreviewContainer.className =
    'w-1/2 h-full border-l border-studio-border flex flex-col bg-studio-sidebar/40 relative hidden';

  const asmPreviewHeader = document.createElement('div');
  asmPreviewHeader.className =
    'h-7 px-3 bg-studio-panel/80 border-b border-studio-border flex items-center justify-between text-[11px] font-mono shrink-0 select-none';
  asmPreviewHeader.innerHTML = `
    <span class="text-blue-400 font-bold flex items-center space-x-1.5"><span>⚡</span><span>M68K ASM (TRANSPILED LIVE)</span></span>
    <span class="text-[10px] text-studio-muted">READ-ONLY</span>
  `;
  const asmPreviewEditorEl = document.createElement('div');
  asmPreviewEditorEl.className = 'flex-1 relative';

  asmPreviewContainer.appendChild(asmPreviewHeader);
  asmPreviewContainer.appendChild(asmPreviewEditorEl);

  editorWrapper.appendChild(editorContainer);
  editorWrapper.appendChild(asmPreviewContainer);

  // Full Tool Views
  const blitterContainer = document.createElement('div');
  blitterContainer.className = 'absolute inset-0 hidden';
  blitterContainer.appendChild(blitterStudio);

  const bitplaneContainer = document.createElement('div');
  bitplaneContainer.className = 'absolute inset-0 hidden';
  bitplaneContainer.appendChild(bitplaneStudio);

  const copperContainer = document.createElement('div');
  copperContainer.className = 'absolute inset-0 hidden';
  copperContainer.appendChild(copperVisualizer.element);

  const memoryMapContainer = document.createElement('div');
  memoryMapContainer.className = 'absolute inset-0 hidden';
  memoryMapContainer.appendChild(memoryMapPanel.element);

  const settingsContainer = document.createElement('div');
  settingsContainer.className = 'absolute inset-0 hidden';
  settingsContainer.appendChild(settingsView.element);

  centerViewsWrapper.appendChild(editorWrapper);
  centerViewsWrapper.appendChild(blitterContainer);
  centerViewsWrapper.appendChild(bitplaneContainer);
  centerViewsWrapper.appendChild(copperContainer);
  centerViewsWrapper.appendChild(memoryMapContainer);
  centerViewsWrapper.appendChild(settingsContainer);

  centerWorkbench.appendChild(centerViewsWrapper);

  // Bottom Multifunction Panel
  centerWorkbench.appendChild(bottomPanel.element);

  workbenchArea.appendChild(centerWorkbench);
  appContainer.appendChild(workbenchArea);

  // Bottom Status Bar
  appContainer.appendChild(statusBar.element);

  // --- Monaco Editor Initialization ---
  const editor = monaco.editor.create(editorContainer, {
    value: currentProject.files[activeFile] || '',
    language: activeFile.endsWith('.py') ? 'python' : 'm68k',
    theme: generalSettings.theme === 'amiga' ? 'amiga-theme' : 'm68k-dark',
    fontSize: generalSettings.editorFontSize || 13,
    tabSize: generalSettings.editorTabSize || 4,
    minimap: { enabled: generalSettings.minimapEnabled },
    automaticLayout: true,
    scrollBeyondLastLine: false,
    lineNumbers: 'on',
    renderLineHighlight: 'all',
    suggestOnTriggerCharacters: true,
    fontFamily: "'JetBrains Mono', 'Fira Code', Menlo, Monaco, Consolas, monospace",
    cursorBlinking: 'smooth',
  });

  // Secondary Read-Only ASM Preview Editor
  const asmPreviewEditor = monaco.editor.create(asmPreviewEditorEl, {
    value: '; Live generierter M68K-Assemblercode...\n',
    language: 'm68k',
    theme: generalSettings.theme === 'amiga' ? 'amiga-theme' : 'm68k-dark',
    fontSize: (generalSettings.editorFontSize || 13) - 1,
    readOnly: true,
    minimap: { enabled: false },
    automaticLayout: true,
    scrollBeyondLastLine: false,
    lineNumbers: 'on',
    fontFamily: "'JetBrains Mono', 'Fira Code', Menlo, Monaco, Consolas, monospace",
  });

  // Track Cursor Line & Column for StatusBar
  editor.onDidChangeCursorPosition((e) => {
    statusBar.updateCursor(e.position.lineNumber, e.position.column);
  });

  // Editor Right Click Context Menu
  editor.onContextMenu((e) => {
    e.event.preventDefault();
    e.event.stopPropagation();
    const mouseEvent = e.event.browserEvent;

    showContextMenu(mouseEvent.clientX, mouseEvent.clientY, [
      {
        id: 'format',
        label: 'Dokument formatieren',
        icon: '🧹',
        shortcut: 'Shift+Alt+F',
        onClick: handleFormat,
      },
      { id: 'sep1', label: '', separator: true },
      {
        id: 'goto_def',
        label: 'Gehe zu Definition',
        icon: '🔍',
        shortcut: 'F12',
        onClick: () => editor.trigger('contextmenu', 'editor.action.revealDefinition', null),
      },
      {
        id: 'find_refs',
        label: 'Alle Referenzen suchen',
        icon: '🔎',
        shortcut: 'Shift+F12',
        onClick: () => editor.trigger('contextmenu', 'editor.action.goToReferences', null),
      },
      { id: 'sep2', label: '', separator: true },
      {
        id: 'cmd_palette',
        label: 'Befehlspalette...',
        icon: '⚡',
        shortcut: 'F1',
        onClick: () => openCommandPalette('commands'),
      },
      {
        id: 'cut',
        label: 'Ausschneiden',
        icon: '✂️',
        shortcut: 'Ctrl+X',
        onClick: () => document.execCommand('cut'),
      },
      {
        id: 'copy',
        label: 'Kopieren',
        icon: '📋',
        shortcut: 'Ctrl+C',
        onClick: () => document.execCommand('copy'),
      },
    ]);
  });

  // Editor Keybindings
  editor.addCommand(monaco.KeyMod.Shift | monaco.KeyMod.Alt | monaco.KeyCode.KeyF, handleFormat);
  editor.addCommand(monaco.KeyCode.F7, handleBuild);
  editor.addCommand(monaco.KeyCode.F5, handleRun);

  // Global Keyboard Shortcuts
  window.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.key.toLowerCase() === 'p') {
      e.preventDefault();
      openCommandPalette('files');
    } else if (e.key === 'F1' || (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'p')) {
      e.preventDefault();
      openCommandPalette('commands');
    } else if (e.ctrlKey && e.key.toLowerCase() === 'b') {
      e.preventDefault();
      isSidebarVisible = !isSidebarVisible;
      updateSidebarVisibility();
    } else if (e.ctrlKey && e.key.toLowerCase() === 'j') {
      e.preventDefault();
      isPanelVisible = !isPanelVisible;
      bottomPanel.element.classList.toggle('hidden', !isPanelVisible);
    } else if (e.key === 'F7') {
      e.preventDefault();
      handleBuild();
    } else if (e.key === 'F5') {
      e.preventDefault();
      handleRun();
    }
  });

  function updateSidebarVisibility() {
    primarySidebarContainer.classList.toggle('hidden', !isSidebarVisible);
    activityBar.setActiveView(currentActivityView, isSidebarVisible);
  }

  function updateSidebarView() {
    viewHolderExplorer.classList.toggle('hidden', currentActivityView !== 'explorer');
    viewHolderSearch.classList.toggle('hidden', currentActivityView !== 'search');
    viewHolderRetro.classList.toggle('hidden', currentActivityView !== 'retro');
    viewHolderDebug.classList.toggle('hidden', currentActivityView !== 'debug');
    updateSidebarVisibility();
  }

  function showCenterView(viewKey: 'editor' | 'blitter' | 'bitplanes' | 'copper' | 'memorymap' | 'settings') {
    [editorWrapper, blitterContainer, bitplaneContainer, copperContainer, memoryMapContainer, settingsContainer].forEach(
      (c) => c.classList.add('hidden')
    );

    if (viewKey === 'editor') {
      editorWrapper.classList.remove('hidden');
      editor.layout();
      if (isAsmPreviewActive) asmPreviewEditor.layout();
    } else if (viewKey === 'blitter') {
      blitterContainer.classList.remove('hidden');
    } else if (viewKey === 'bitplanes') {
      bitplaneContainer.classList.remove('hidden');
    } else if (viewKey === 'copper') {
      copperContainer.classList.remove('hidden');
      if (currentBinary.length > 0) {
        api.parseCopper(currentBinary).then((res) => copperVisualizer.updateInstructions(res.instructions));
      }
    } else if (viewKey === 'memorymap') {
      memoryMapContainer.classList.remove('hidden');
    } else if (viewKey === 'settings') {
      settingsContainer.classList.remove('hidden');
    }
  }

  function switchToFile(path: string) {
    if (path.startsWith('tool:')) {
      activeFile = path;
      if (!openFiles.includes(path)) openFiles.push(path);
      editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
      showCenterView(path.replace('tool:', '') as any);
      return;
    }

    // Save current file
    if (!activeFile.startsWith('tool:') && currentProject.files[activeFile] !== undefined) {
      currentProject.files[activeFile] = editor.getValue();
    }

    activeFile = path;
    if (!openFiles.includes(path)) openFiles.push(path);

    if (currentProject.files[path] !== undefined) {
      editor.setValue(currentProject.files[path]);
    }

    const model = editor.getModel();
    if (model) {
      monaco.editor.setModelLanguage(model, path.endsWith('.py') ? 'python' : 'm68k');
    }

    statusBar.updateLanguage(path.endsWith('.py') ? 'python' : 'm68k');
    editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
    projectExplorer.setActiveFile(activeFile);
    saveCurrentProject(currentProject);
    showCenterView('editor');
    updateDiagnostics();
    updateAsmPreview();
  }

  async function updateAsmPreview() {
    if (!isAsmPreviewActive || !activeFile.endsWith('.py') || activeFile.startsWith('tool:')) {
      asmPreviewContainer.classList.add('hidden');
      editor.layout();
      return;
    }

    asmPreviewContainer.classList.remove('hidden');
    editor.layout();
    asmPreviewEditor.layout();

    try {
      const code = editor.getValue();
      const res = await api.transpilePython(code, currentProfile.system, activeCpu);
      if (res && res.asm_code) {
        asmPreviewEditor.setValue(res.asm_code);
      }
    } catch (err) {
      asmPreviewEditor.setValue(`; Fehler bei der Live-Transpilierung:\n; ${err}`);
    }
  }

  function applySettingsToEditor(s: AppGeneralSettings) {
    editor.updateOptions({
      fontSize: s.editorFontSize,
      tabSize: s.editorTabSize,
      minimap: { enabled: s.minimapEnabled },
    });
    asmPreviewEditor.updateOptions({
      fontSize: (s.editorFontSize || 13) - 1,
      tabSize: s.editorTabSize,
    });
    if (s.theme === 'amiga') {
      document.body.classList.add('theme-amiga');
      monaco.editor.setTheme('amiga-theme');
    } else {
      document.body.classList.remove('theme-amiga');
      monaco.editor.setTheme('m68k-dark');
    }
  }

  function applyPlatformProfile(profileId: string) {
    currentProfile = getActiveProfile(profileId);
    activeCpu = currentProfile.targetCpu;
    currentProject.profileId = currentProfile.id;
    saveCurrentProject(currentProject);

    titleBar.updateActiveProfile(profileId);
    runDebugView.updateProfile(currentProfile);
    sidebarFilesContainer.updateProfile(currentProfile);
    statusBar.updateProfile(currentProfile.name, activeCpu);

    sidebarFilesContainer.setCapabilities(true);
    retroStudioView.setCapabilities(
      currentProfile.capabilities.hasBlitter,
      currentProfile.capabilities.hasCopper,
      currentProfile.capabilities.hasBitplanes
    );

    memoryMapPanel.updateStats(
      0,
      0,
      0,
      currentProfile.memory.ramTotalKb,
      currentProfile.memory.ramLabel,
      `Motorola ${activeCpu}`
    );

    bottomPanel.log(`Active Target Profile: ${currentProfile.name} (CPU: ${activeCpu})`);
    updateDiagnostics();
    updateAsmPreview();
  }

  function openCommandPalette(mode: 'commands' | 'files') {
    const commands: CommandItem[] = [
      { id: 'build', title: 'Build Project', category: 'Build', shortcut: 'F7', icon: '🔨', run: handleBuild },
      { id: 'run', title: 'Run in Emulator', category: 'Run', shortcut: 'F5', icon: '▶', run: handleRun },
      { id: 'format', title: 'Format Document', category: 'Source', shortcut: 'Shift+Alt+F', icon: '🧹', run: handleFormat },
      { id: 'new_proj', title: 'New Project from Template...', category: 'Project', icon: '📦', run: openNewProjectWizard },
      { id: 'tool_blitter', title: 'Open Blitter Studio', category: 'Tools', icon: '🧮', run: () => switchToFile('tool:blitter') },
      { id: 'tool_bitplanes', title: 'Open Bitplane Studio', category: 'Tools', icon: '🎨', run: () => switchToFile('tool:bitplanes') },
      { id: 'tool_copper', title: 'Open Copperlist Visualizer', category: 'Tools', icon: '🌈', run: () => switchToFile('tool:copper') },
      { id: 'tool_memorymap', title: 'Open Memory Map', category: 'Tools', icon: '🗺️', run: () => switchToFile('tool:memorymap') },
      { id: 'tool_hwregs', title: 'Open Hardware Register DB', category: 'Tools', icon: '⚙️', run: () => { isPanelVisible = true; bottomPanel.element.classList.remove('hidden'); bottomPanel.setTab('hwregs'); } },
      { id: 'settings', title: 'Open Settings & Profiles', category: 'Preferences', shortcut: 'Ctrl+,', icon: '⚙️', run: () => switchToFile('tool:settings') },
      { id: 'toggle_sidebar', title: 'Toggle Primary Sidebar', category: 'View', shortcut: 'Ctrl+B', icon: '📁', run: () => { isSidebarVisible = !isSidebarVisible; updateSidebarVisibility(); } },
      { id: 'toggle_panel', title: 'Toggle Bottom Panel', category: 'View', shortcut: 'Ctrl+J', icon: '🔽', run: () => { isPanelVisible = !isPanelVisible; bottomPanel.element.classList.toggle('hidden', !isPanelVisible); } },
      { id: 'theme', title: 'Toggle Theme (Dark / Amiga Retro)', category: 'Preferences', icon: '🎨', run: () => document.body.classList.toggle('theme-amiga') },
    ];

    const files = Object.keys(currentProject.files).map((p) => ({
      path: p,
      name: p.split('/').pop() || p,
    }));

    showCommandPalette({
      mode,
      files,
      commands,
      onFileSelect: switchToFile,
    });
  }

  /// Creating or opening a project means leaving the current one, so it
  /// goes back to the start screen — which owns both flows and writes to
  /// disk. The old in-memory wizard could not create a real project.
  function openNewProjectWizard() {
    forgetLastProject();
    showStartScreen();
  }

  function exportProjectAsJson() {
    const jsonStr = JSON.stringify(currentProject, null, 2);
    const blob = new Blob([jsonStr], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${currentProject.name.toLowerCase()}-project.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast(`Projekt als JSON exportiert.`, 'success');
  }

  function downloadTargetArtifact() {
    if (currentBinary.length === 0) {
      showToast('Bitte erst das Projekt kompilieren (F7).', 'info');
      return;
    }

    let bytes: Uint8Array;
    let filename = `${currentProject.name.toLowerCase()}`;

    if (currentProfile.capabilities.hasAdfFloppy && currentAdfBytes.length > 0) {
      bytes = new Uint8Array(currentAdfBytes);
      filename += '.adf';
    } else if (currentProfile.system === 'megadrive') {
      bytes = new Uint8Array(currentBinary);
      filename += '.bin';
    } else if (currentProfile.system === 'atarist') {
      bytes = new Uint8Array(currentBinary);
      filename += '.prg';
    } else {
      bytes = new Uint8Array(currentBinary);
      filename += '.bin';
    }

    const blob = new Blob([bytes as any], { type: 'application/octet-stream' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast(`Target-Datei "${filename}" heruntergeladen.`, 'success');
  }

  // Diagnostics / LSP Bridge
  async function updateDiagnostics() {
    if (activeFile.startsWith('tool:')) return;
    const source = editor.getValue();
    const diags: IdeDiagnosticItem[] = await api.lspDiagnostics(source, activeCpu);

    const markers: monaco.editor.IMarkerData[] = diags.map((d) => ({
      severity:
        d.severity === 1
          ? monaco.MarkerSeverity.Error
          : d.severity === 2
          ? monaco.MarkerSeverity.Warning
          : monaco.MarkerSeverity.Info,
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

    const errCount = diags.filter((d) => d.severity === 1).length;
    const warnCount = diags.filter((d) => d.severity !== 1).length;
    statusBar.updateProblemsCount(errCount, warnCount);
    bottomPanel.setProblems([], [], diags);

    // Symbols outline
    const symbols = await api.lspSymbols(source);
    sidebarFilesContainer.updateSymbols(symbols);

    // Cycle Analysis
    const cyclesRes = await api.analyzeCycles(source, activeCpu);
    const cycleStr = `⚡ ~${cyclesRes.total_min_cycles} Cycles | ${cyclesRes.pal_scanlines.toFixed(1)} PAL Lines`;
    statusBar.updateCycles(cycleStr);
  }

  let diagTimer: any = null;
  editor.onDidChangeModelContent(() => {
    if (activeFile.startsWith('tool:')) return;
    dirtyFiles.add(activeFile);
    editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
    clearTimeout(diagTimer);
    diagTimer = setTimeout(() => {
      updateDiagnostics();
      updateAsmPreview();
      dirtyFiles.delete(activeFile);
      editorTabs.updateTabs(openFiles, activeFile, dirtyFiles, isAsmPreviewActive);
    }, 250);
  });

  // Build Handler
  async function handleBuild() {
    bottomPanel.log(`Building project "${currentProject.name}" for ${currentProfile.name} (CPU: ${activeCpu})...`);
    if (!activeFile.startsWith('tool:')) {
      const source = editor.getValue();
      currentProject.files[activeFile] = source;
      saveCurrentProject(currentProject);
    }

    const entryFile = currentProject.mainFile || activeFile;
    const source = currentProject.files[entryFile] || editor.getValue();

    const res: AssembleResponse = await api.assemble({
      source,
      cpu: activeCpu,
      platform: currentProfile.system,
    });

    bottomPanel.setProblems(res.errors, res.warnings);
    statusBar.updateProblemsCount(res.errors.length, res.warnings.length);

    if (res.success && res.binary_base64) {
      const binaryString = atob(res.binary_base64);
      currentBinary = Array.from(binaryString, (c) => c.charCodeAt(0));

      if (res.generated_asm) {
        bottomPanel.log(`🐍 Python successfully transpiled to m68k assembly! (${res.byte_count} bytes)`);
        showToast(`Python zu m68k kompiliert (${res.byte_count} Bytes)`, 'success');
      } else {
        bottomPanel.log(`Assembly successful! Generated ${res.byte_count} bytes.`);
        showToast(`Erfolgreich assembliert (${res.byte_count} Bytes)`, 'success');
      }

      // Update Disassembly View
      const disasmRes = await api.disassemble(currentBinary, 0, activeCpu);
      disasmPanel.updateLines(disasmRes.lines);

      // Update Hex Memory Viewer
      hexMemoryViewer.updateData(currentBinary, 0);

      // Auto-generate virtual ADF for Amiga profiles
      if (currentProfile.capabilities.hasAdfFloppy) {
        const adfBytes = await api.createAdf('AmigaDemo', false, currentBinary);
        if (adfBytes && adfBytes.length > 0) {
          currentAdfBytes = adfBytes;
          const inspectRes = await api.inspectAdf(currentAdfBytes);
          if (inspectRes) {
            sidebarFilesContainer.updateAdf(inspectRes);
          }
          bottomPanel.log('Created bootable Amiga OFS floppy disk image (880 KB).');
        }
      }

      // Update Memory Map Footprint
      memoryMapPanel.updateStats(
        res.byte_count,
        0,
        0,
        currentProfile.memory.ramTotalKb,
        currentProfile.memory.ramLabel,
        `Motorola ${activeCpu}`
      );
    } else {
      bottomPanel.log('Build failed with errors.', true);
      showToast('Kompilierung fehlgeschlagen.', 'error');
      bottomPanel.setTab('problems');
    }
  }

  // Run Handler (Emulator Launch)
  async function handleRun() {
    await handleBuild();
    if (currentBinary.length === 0) {
      bottomPanel.log('Cannot run: Binary is empty.', true);
      return;
    }

    const emuType = currentProfile.defaultEmulator;
    bottomPanel.log(`Launching ${emuType} for profile ${currentProfile.name}...`);

    const payloadBytes = currentAdfBytes.length > 0 ? currentAdfBytes : currentBinary;
    const payloadB64 = btoa(String.fromCharCode(...payloadBytes));

    const launchRes = await api.launchEmulator({
      emulator_type: emuType,
      target_file_name: currentAdfBytes.length > 0 ? 'demo.adf' : 'rom.bin',
      target_file_data_base64: payloadB64,
      custom_executable: currentProfile.customExecutable,
      kickstart_rom_path: currentProfile.customRomPath,
      extra_args: currentProfile.customArgs ? currentProfile.customArgs.split(' ').filter(Boolean) : undefined,
    });

    if (launchRes.success) {
      bottomPanel.log(`Emulator launched: ${launchRes.message}`);
      showToast(launchRes.message, 'success');
    } else {
      bottomPanel.log(`Failed to spawn emulator: ${launchRes.message}`, true);
      showToast(`Emulator-Start fehlgeschlagen: ${launchRes.message}`, 'error');
    }
  }

  // Format Handler
  async function handleFormat() {
    if (activeFile.startsWith('tool:')) return;
    const source = editor.getValue();
    const formatted = await api.lspFormat(source);
    if (formatted && formatted !== source) {
      editor.setValue(formatted);
      currentProject.files[activeFile] = formatted;
      saveCurrentProject(currentProject);
      bottomPanel.log('Source code formatted via m68k LSP formatter.');
      showToast('Code erfolgreich formatiert!', 'info');
    }
  }

  // Initial Diagnostics & Setup
  applyPlatformProfile(currentProject.profileId || 'amiga500');
  setTimeout(updateDiagnostics, 100);
}

/// Show the start screen until a project is opened.
///
/// No project is invented on the user's behalf: an IDE that greets a
/// developer with generated demo code makes them clear it away before
/// they can reach their own work.
function showStartScreen() {
  const appContainer = document.getElementById('app') as HTMLElement;
  appContainer.innerHTML = '';
  appContainer.appendChild(
    createStartScreen({
      onProjectOpened: (project) => {
        rememberLastProject(project.root);
        void initApp(project);
      },
    })
  );
}

/// Reopen the last project if it is still there, otherwise start fresh.
async function boot() {
  const last = lastProjectRoot();
  if (last) {
    try {
      const project = await workspace.open(last);
      void initApp(project);
      return;
    } catch {
      // Moved, deleted, or no longer a project — fall through to the
      // start screen rather than failing on an empty window.
      forgetLastProject();
    }
  }
  showStartScreen();
}

window.addEventListener('DOMContentLoaded', () => void boot());
