//! Studio-grade Context Menu Component for m68k Studio.

export interface ContextMenuItem {
  id: string;
  label: string;
  icon?: string;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  separator?: boolean;
  onClick?: () => void;
}

let activeContextMenu: HTMLElement | null = null;
let dismissListener: ((e: MouseEvent) => void) | null = null;

export function dismissContextMenu() {
  if (activeContextMenu && document.body.contains(activeContextMenu)) {
    activeContextMenu.remove();
    activeContextMenu = null;
  }
  if (dismissListener) {
    document.removeEventListener('pointerdown', dismissListener);
    dismissListener = null;
  }
}

export function showContextMenu(x: number, y: number, items: ContextMenuItem[]) {
  dismissContextMenu();

  const menu = document.createElement('div');
  menu.id = 'studio-context-menu';
  menu.className =
    'fixed z-[10000] bg-studio-panel/95 backdrop-blur-md border border-studio-border rounded-xl shadow-2xl py-1.5 min-w-[210px] text-xs font-sans select-none animate-fade-in';

  // Prevent context menu from opening browser context menu
  menu.addEventListener('contextmenu', (e) => e.preventDefault());

  for (const item of items) {
    if (item.separator) {
      const sep = document.createElement('div');
      sep.className = 'h-px bg-studio-border my-1 mx-2';
      menu.appendChild(sep);
      continue;
    }

    const btn = document.createElement('button');
    btn.className = `w-full text-left px-3 py-1.5 flex items-center justify-between transition cursor-pointer ${
      item.disabled
        ? 'text-studio-muted/40 cursor-not-allowed'
        : item.danger
        ? 'text-red-400 hover:bg-red-500/20 hover:text-red-300'
        : 'text-studio-text hover:bg-studio-hover hover:text-white'
    }`;

    const iconHtml = item.icon ? `<span class="shrink-0 text-sm w-4 text-center mr-2">${item.icon}</span>` : '<span class="w-4 mr-2"></span>';
    const labelHtml = `<span class="font-medium truncate">${item.label}</span>`;
    const shortcutHtml = item.shortcut
      ? `<span class="text-[10px] font-mono text-studio-muted shrink-0 ml-3 bg-studio-bg/60 px-1.5 py-0.5 rounded border border-studio-border/50">${item.shortcut}</span>`
      : '';

    btn.innerHTML = `
      <div class="flex items-center min-w-0">
        ${iconHtml}
        ${labelHtml}
      </div>
      ${shortcutHtml}
    `;

    if (!item.disabled && item.onClick) {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        dismissContextMenu();
        item.onClick!();
      });
    }

    menu.appendChild(btn);
  }

  document.body.appendChild(menu);

  // Position within viewport boundaries
  const rect = menu.getBoundingClientRect();
  let finalX = x;
  let finalY = y;

  if (finalX + rect.width > window.innerWidth - 10) {
    finalX = window.innerWidth - rect.width - 10;
  }
  if (finalY + rect.height > window.innerHeight - 10) {
    finalY = window.innerHeight - rect.height - 10;
  }

  menu.style.left = `${Math.max(10, finalX)}px`;
  menu.style.top = `${Math.max(10, finalY)}px`;

  activeContextMenu = menu;

  // Auto-dismiss on click outside
  dismissListener = (e: MouseEvent) => {
    if (!menu.contains(e.target as Node)) {
      dismissContextMenu();
    }
  };

  setTimeout(() => {
    document.addEventListener('pointerdown', dismissListener!);
  }, 10);
}
