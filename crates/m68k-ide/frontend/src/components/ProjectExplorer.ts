import { StudioProject, BUILTIN_FILE_TEMPLATES } from '../project';
import { showContextMenu, ContextMenuItem } from './ContextMenu';
import { showToast, showPromptDialog, showConfirmDialog } from './NotificationSystem';

export interface ProjectExplorerOptions {
  project: StudioProject;
  activeFile: string;
  onFileSelect: (path: string) => void;
  onFileCreate: (path: string, initialContent?: string) => void;
  onFileRename: (oldPath: string, newPath: string) => void;
  onFileDelete: (path: string) => void;
  onSetMainFile: (path: string) => void;
  onNewProjectWizard: () => void;
  onExportProject: () => void;
}

interface TreeNode {
  name: string;
  path: string;
  isFolder: boolean;
  children: Map<string, TreeNode>;
}

export function createProjectExplorer(options: ProjectExplorerOptions) {
  const container = document.createElement('div');
  container.className = 'h-full flex flex-col bg-studio-bg text-studio-text select-none text-xs font-sans';

  let currentProject = options.project;
  let currentActiveFile = options.activeFile;
  const collapsedFolders = new Set<string>();

  container.innerHTML = `
    <!-- Explorer Top Header -->
    <div class="px-3 py-2.5 border-b border-studio-border bg-studio-panel/70 flex items-center justify-between shrink-0">
      <div class="flex items-center space-x-1.5 min-w-0">
        <span class="text-sm">📂</span>
        <span class="font-bold text-xs text-white uppercase tracking-wider truncate" id="explorer-proj-name">${currentProject.name}</span>
      </div>
      <!-- Quick Action Icons -->
      <div class="flex items-center space-x-1">
        <button id="exp-btn-new-file" class="p-1 text-studio-muted hover:text-white hover:bg-studio-hover rounded transition cursor-pointer" title="Neue Datei erstellen">
          ➕
        </button>
        <button id="exp-btn-new-folder" class="p-1 text-studio-muted hover:text-white hover:bg-studio-hover rounded transition cursor-pointer" title="Neuen Ordner erstellen">
          📁
        </button>
        <button id="exp-btn-wizard" class="p-1 text-studio-muted hover:text-white hover:bg-studio-hover rounded transition cursor-pointer" title="Projekt-Vorlagen & Wizard">
          📦
        </button>
        <button id="exp-btn-collapse" class="p-1 text-studio-muted hover:text-white hover:bg-studio-hover rounded transition cursor-pointer" title="Alle Ordner einklappen">
          🔽
        </button>
      </div>
    </div>

    <!-- Tree View Scroll Container -->
    <div class="flex-1 overflow-y-auto p-1.5 space-y-0.5" id="explorer-tree-container">
      <!-- Dynamically generated tree nodes -->
    </div>

    <!-- Explorer Footer / Project Meta -->
    <div class="p-2 border-t border-studio-border bg-studio-panel/40 flex items-center justify-between text-[10px] text-studio-muted shrink-0">
      <span>${Object.keys(currentProject.files).length} Dateien</span>
      <button id="exp-btn-export" class="hover:text-white transition flex items-center space-x-1 cursor-pointer">
        <span>💾 Export</span>
      </button>
    </div>
  `;

  // Build Tree from flat file paths
  function buildTree(): TreeNode {
    const root: TreeNode = {
      name: '',
      path: '',
      isFolder: true,
      children: new Map(),
    };

    for (const filePath of Object.keys(currentProject.files)) {
      const parts = filePath.split('/').filter(Boolean);
      let current = root;

      for (let i = 0; i < parts.length; i++) {
        const part = parts[i];
        const isFile = i === parts.length - 1;
        const currentPath = parts.slice(0, i + 1).join('/');

        if (!current.children.has(part)) {
          current.children.set(part, {
            name: part,
            path: currentPath,
            isFolder: !isFile,
            children: new Map(),
          });
        }
        current = current.children.get(part)!;
      }
    }

    return root;
  }

  // Get file icon based on extension
  function getFileIcon(name: string): string {
    if (name.endsWith('.py')) return '🐍';
    if (name.endsWith('.s') || name.endsWith('.asm')) return '⚙️';
    if (name.endsWith('.i') || name.endsWith('.inc') || name.endsWith('.h')) return '📄';
    if (name.endsWith('.png') || name.endsWith('.bmp') || name.endsWith('.iff')) return '🖼️';
    if (name.endsWith('.adf') || name.endsWith('.bin') || name.endsWith('.rom')) return '💾';
    if (name.endsWith('.json')) return '🔧';
    if (name.endsWith('.md') || name.endsWith('.txt')) return '📝';
    return '📄';
  }

  // Render tree node recursive HTML
  function renderTreeNode(node: TreeNode, depth: number = 0): string {
    if (!node.path) {
      // Root level: sort folders first, then files
      const entries = Array.from(node.children.values()).sort((a, b) => {
        if (a.isFolder === b.isFolder) return a.name.localeCompare(b.name);
        return a.isFolder ? -1 : 1;
      });
      return entries.map((child) => renderTreeNode(child, 0)).join('');
    }

    const paddingLeft = `${depth * 14 + 6}px`;

    if (node.isFolder) {
      const isCollapsed = collapsedFolders.has(node.path);
      const childEntries = Array.from(node.children.values()).sort((a, b) => {
        if (a.isFolder === b.isFolder) return a.name.localeCompare(b.name);
        return a.isFolder ? -1 : 1;
      });

      return `
        <div class="tree-folder-group" data-folderpath="${node.path}">
          <div class="tree-folder-row flex items-center px-2 py-1 rounded-md hover:bg-studio-hover text-studio-text hover:text-white transition cursor-pointer group" style="padding-left: ${paddingLeft};" data-folderpath="${node.path}">
            <span class="w-3.5 text-center text-[9px] text-studio-muted group-hover:text-white mr-1 transition transform ${
              isCollapsed ? '-rotate-90' : ''
            }">▼</span>
            <span class="text-xs mr-1.5">📁</span>
            <span class="font-semibold text-xs truncate">${node.name}</span>
          </div>
          <div class="tree-folder-children ${isCollapsed ? 'hidden' : ''}">
            ${childEntries.map((child) => renderTreeNode(child, depth + 1)).join('')}
          </div>
        </div>
      `;
    } else {
      const isSelected = node.path === currentActiveFile;
      const isMain = node.path === currentProject.mainFile;
      const icon = getFileIcon(node.name);

      return `
        <div class="tree-file-row flex items-center justify-between px-2 py-1 rounded-md transition cursor-pointer group ${
          isSelected
            ? 'bg-blue-600/20 text-blue-400 font-semibold border-l-2 border-blue-500'
            : 'text-studio-text hover:bg-studio-hover hover:text-white'
        }" style="padding-left: ${paddingLeft};" data-filepath="${node.path}">
          <div class="flex items-center space-x-1.5 truncate min-w-0">
            <span class="text-xs shrink-0">${icon}</span>
            <span class="truncate text-xs">${node.name}</span>
          </div>
          ${
            isMain
              ? '<span class="text-[9px] px-1 py-0.2 rounded bg-amber-500/20 text-amber-300 font-bold tracking-wider shrink-0 ml-1">MAIN</span>'
              : ''
          }
        </div>
      `;
    }
  }

  // Render whole tree and wire click/contextmenu events
  function renderTree() {
    const treeContainer = container.querySelector('#explorer-tree-container');
    if (!treeContainer) return;

    const root = buildTree();
    treeContainer.innerHTML = renderTreeNode(root);

    // Folder click (toggle collapse)
    treeContainer.querySelectorAll('.tree-folder-row').forEach((folderRow) => {
      folderRow.addEventListener('click', (e) => {
        e.stopPropagation();
        const fPath = folderRow.getAttribute('data-folderpath');
        if (fPath) {
          if (collapsedFolders.has(fPath)) {
            collapsedFolders.delete(fPath);
          } else {
            collapsedFolders.add(fPath);
          }
          renderTree();
        }
      });

      // Folder Context Menu (Right Click)
      folderRow.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        const mouseEvent = e as MouseEvent;
        const fPath = folderRow.getAttribute('data-folderpath') || '';
        showFolderContextMenu(mouseEvent.clientX, mouseEvent.clientY, fPath);
      });
    });

    // File click (select file)
    treeContainer.querySelectorAll('.tree-file-row').forEach((fileRow) => {
      fileRow.addEventListener('click', (e) => {
        e.stopPropagation();
        const fPath = fileRow.getAttribute('data-filepath');
        if (fPath) {
          currentActiveFile = fPath;
          options.onFileSelect(fPath);
          renderTree();
        }
      });

      // File Context Menu (Right Click)
      fileRow.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        const mouseEvent = e as MouseEvent;
        const fPath = fileRow.getAttribute('data-filepath') || '';
        showFileContextMenu(mouseEvent.clientX, mouseEvent.clientY, fPath);
      });
    });

    // Background Right Click (Explorer root)
    treeContainer.addEventListener('contextmenu', (e) => {
      if ((e.target as HTMLElement).closest('.tree-file-row') || (e.target as HTMLElement).closest('.tree-folder-row')) {
        return;
      }
      e.preventDefault();
      const mouseEvent = e as MouseEvent;
      showRootContextMenu(mouseEvent.clientX, mouseEvent.clientY);
    });
  }

  // --- Context Menus ---

  function showFileContextMenu(x: number, y: number, filePath: string) {
    const isMain = filePath === currentProject.mainFile;
    const isProjectJson = filePath === 'project.json';

    const items: ContextMenuItem[] = [
      {
        id: 'open',
        label: 'Öffnen',
        icon: '📄',
        onClick: () => {
          currentActiveFile = filePath;
          options.onFileSelect(filePath);
          renderTree();
        },
      },
      {
        id: 'set_main',
        label: 'Als Haupt-Einstiegsdatei (Main) setzen',
        icon: '⚡',
        disabled: isMain || isProjectJson,
        onClick: () => {
          options.onSetMainFile(filePath);
          showToast(`"${filePath}" als Main File gesetzt.`, 'success');
          renderTree();
        },
      },
      { id: 'sep1', label: '', separator: true },
      {
        id: 'new_from_template',
        label: 'Neue Datei aus Vorlage...',
        icon: '✨',
        onClick: () => showFileTemplateSelector(filePath.includes('/') ? filePath.substring(0, filePath.lastIndexOf('/')) : ''),
      },
      {
        id: 'rename',
        label: 'Umbenennen (F2)',
        icon: '✏️',
        shortcut: 'F2',
        disabled: isProjectJson,
        onClick: () => {
          showPromptDialog({
            title: 'Datei umbenennen',
            defaultValue: filePath,
            confirmText: 'Umbenennen',
            onConfirm: (newPath) => {
              if (newPath && newPath !== filePath) {
                options.onFileRename(filePath, newPath);
                renderTree();
              }
            },
          });
        },
      },
      {
        id: 'copy_path',
        label: 'Pfad kopieren',
        icon: '📋',
        onClick: () => {
          navigator.clipboard.writeText(filePath);
          showToast(`Pfad kopiert: ${filePath}`, 'info');
        },
      },
      { id: 'sep2', label: '', separator: true },
      {
        id: 'delete',
        label: 'Löschen (Entf)',
        icon: '🗑️',
        danger: true,
        disabled: isProjectJson,
        onClick: () => {
          showConfirmDialog({
            title: 'Datei löschen',
            message: `Möchtest du "${filePath}" wirklich unwiderruflich löschen?`,
            confirmText: 'Löschen',
            isDanger: true,
            onConfirm: () => {
              options.onFileDelete(filePath);
              renderTree();
            },
          });
        },
      },
    ];

    showContextMenu(x, y, items);
  }

  function showFolderContextMenu(x: number, y: number, folderPath: string) {
    const items: ContextMenuItem[] = [
      {
        id: 'new_file',
        label: 'Neue Datei in diesem Ordner...',
        icon: '➕',
        onClick: () => {
          showPromptDialog({
            title: 'Neue Datei',
            message: `Neue Datei im Ordner "${folderPath}/" erstellen:`,
            placeholder: `${folderPath}/routine.s`,
            defaultValue: `${folderPath}/`,
            confirmText: 'Erstellen',
            onConfirm: (path) => {
              if (path) options.onFileCreate(path);
            },
          });
        },
      },
      {
        id: 'new_from_template',
        label: 'Neue Datei aus Vorlage...',
        icon: '✨',
        onClick: () => showFileTemplateSelector(folderPath),
      },
      { id: 'sep1', label: '', separator: true },
      {
        id: 'new_subfolder',
        label: 'Neuer Unterordner...',
        icon: '📁',
        onClick: () => {
          showPromptDialog({
            title: 'Neuer Unterordner',
            placeholder: `${folderPath}/subfolder`,
            defaultValue: `${folderPath}/`,
            confirmText: 'Erstellen',
            onConfirm: (sub) => {
              if (sub) options.onFileCreate(`${sub.replace(/\/$/, '')}/.keep`, '');
            },
          });
        },
      },
      {
        id: 'copy_folder_path',
        label: 'Ordnerpfad kopieren',
        icon: '📋',
        onClick: () => {
          navigator.clipboard.writeText(folderPath);
          showToast(`Ordnerpfad kopiert: ${folderPath}`, 'info');
        },
      },
    ];

    showContextMenu(x, y, items);
  }

  function showRootContextMenu(x: number, y: number) {
    const items: ContextMenuItem[] = [
      {
        id: 'new_file',
        label: 'Neue Datei erstellen...',
        icon: '➕',
        onClick: () => {
          showPromptDialog({
            title: 'Neue Quellcode-Datei',
            placeholder: 'src/main.py',
            defaultValue: 'src/',
            confirmText: 'Erstellen',
            onConfirm: (path) => {
              if (path) options.onFileCreate(path);
            },
          });
        },
      },
      {
        id: 'new_folder',
        label: 'Neuer Ordner...',
        icon: '📁',
        onClick: () => {
          showPromptDialog({
            title: 'Neuer Ordner',
            placeholder: 'assets',
            confirmText: 'Erstellen',
            onConfirm: (folder) => {
              if (folder) options.onFileCreate(`${folder.replace(/\/$/, '')}/.keep`, '');
            },
          });
        },
      },
      {
        id: 'new_from_tpl',
        label: 'Neue Datei aus Vorlage...',
        icon: '✨',
        onClick: () => showFileTemplateSelector('src'),
      },
      { id: 'sep1', label: '', separator: true },
      {
        id: 'wizard',
        label: 'Neues Projekt aus Vorlage (Wizard)...',
        icon: '📦',
        onClick: options.onNewProjectWizard,
      },
      {
        id: 'export_proj',
        label: 'Projekt exportieren...',
        icon: '💾',
        onClick: options.onExportProject,
      },
    ];

    showContextMenu(x, y, items);
  }

  // Dialog for inserting a file from BUILTIN_FILE_TEMPLATES
  function showFileTemplateSelector(targetDir: string) {
    const overlay = document.createElement('div');
    overlay.className =
      'fixed inset-0 z-[9999] bg-black/80 backdrop-blur-sm flex items-center justify-center p-4 select-none';

    overlay.innerHTML = `
      <div class="bg-studio-panel border border-studio-border rounded-2xl max-w-lg w-full p-6 space-y-4 shadow-2xl animate-fade-in text-xs font-sans">
        <div class="flex items-center justify-between border-b border-studio-border pb-3">
          <div class="flex items-center space-x-2.5">
            <span class="text-xl">✨</span>
            <h3 class="text-sm font-bold text-white">Neue Datei aus Vorlage einfügen</h3>
          </div>
          <button id="ftpl-btn-close" class="text-studio-muted hover:text-white p-1 cursor-pointer">✕</button>
        </div>
        <div class="space-y-2 max-h-80 overflow-y-auto pr-1">
          ${BUILTIN_FILE_TEMPLATES.map(
            (ft) => `
            <div class="file-tpl-card p-3 rounded-xl border border-studio-border/60 bg-studio-bg/60 hover:bg-studio-bg hover:border-blue-500 transition cursor-pointer" data-ftid="${ft.id}">
              <div class="flex items-center justify-between font-bold text-white text-xs">
                <span>${ft.name}</span>
                <span class="px-1.5 py-0.5 rounded text-[9px] font-mono uppercase ${
                  ft.language === 'python' ? 'bg-amber-500/20 text-amber-300' : 'bg-blue-500/20 text-blue-300'
                }">${ft.language}</span>
              </div>
              <p class="text-[10px] text-studio-muted mt-1 leading-relaxed">${ft.description}</p>
            </div>
          `
          ).join('')}
        </div>
      </div>
    `;

    document.body.appendChild(overlay);

    const close = () => overlay.remove();
    overlay.querySelector('#ftpl-btn-close')?.addEventListener('click', close);

    overlay.querySelectorAll('.file-tpl-card').forEach((card) => {
      card.addEventListener('click', () => {
        const ftid = card.getAttribute('data-ftid');
        const ft = BUILTIN_FILE_TEMPLATES.find((t) => t.id === ftid);
        if (!ft) return;
        close();

        const defaultFileName = `${targetDir ? targetDir.replace(/\/$/, '') + '/' : ''}${ft.id.replace('file_', '')}${ft.extension}`;
        showPromptDialog({
          title: 'Dateinamen festlegen',
          defaultValue: defaultFileName,
          confirmText: 'Einfügen',
          onConfirm: (path) => {
            if (path) {
              options.onFileCreate(path, ft.content);
              showToast(`Vorlage "${ft.name}" eingefügt!`, 'success');
            }
          },
        });
      });
    });
  }

  // --- Top Header Buttons ---
  container.querySelector('#exp-btn-new-file')?.addEventListener('click', () => {
    showPromptDialog({
      title: 'Neue Datei erstellen',
      placeholder: 'src/main.py',
      defaultValue: 'src/',
      confirmText: 'Erstellen',
      onConfirm: (path) => {
        if (path) options.onFileCreate(path);
      },
    });
  });

  container.querySelector('#exp-btn-new-folder')?.addEventListener('click', () => {
    showPromptDialog({
      title: 'Neuen Ordner erstellen',
      placeholder: 'assets',
      confirmText: 'Erstellen',
      onConfirm: (folder) => {
        if (folder) options.onFileCreate(`${folder.replace(/\/$/, '')}/.keep`, '');
      },
    });
  });

  container.querySelector('#exp-btn-wizard')?.addEventListener('click', options.onNewProjectWizard);
  container.querySelector('#exp-btn-export')?.addEventListener('click', options.onExportProject);

  container.querySelector('#exp-btn-collapse')?.addEventListener('click', () => {
    // Collapse all folders
    const root = buildTree();
    function collectFolders(node: TreeNode) {
      if (node.isFolder && node.path) collapsedFolders.add(node.path);
      node.children.forEach(collectFolders);
    }
    collectFolders(root);
    renderTree();
  });

  // Initial render
  renderTree();

  return {
    element: container,
    updateProject: (newProject: StudioProject, activeFile: string) => {
      currentProject = newProject;
      currentActiveFile = activeFile;
      const titleEl = container.querySelector('#explorer-proj-name');
      if (titleEl) titleEl.textContent = newProject.name;
      renderTree();
    },
    setActiveFile: (path: string) => {
      currentActiveFile = path;
      renderTree();
    },
  };
}
