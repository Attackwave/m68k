//! Platform & Target Profile System for m68k Studio.

export interface PlatformCapabilities {
  hasCopper: boolean;
  hasBlitter: boolean;
  hasBitplanes: boolean;
  hasAdfFloppy: boolean;
  hasVdpTiles: boolean;
}

export interface PlatformMemoryConfig {
  ramTotalKb: number;
  ramLabel: string;
  romTotalKb: number;
  romLabel: string;
}

export interface ProfileBuildOptions {
  defaultOrigin: string;
  defaultOutputFormat: string; // 'adf' | 'bin' | 'srec' | 'hex' | 'elf' | 'ieee695'
  branchRelaxation: 'auto' | 'strict';
  optimizeImmediates: boolean;
  generateMapFile: boolean;
}

export interface PlatformProfile {
  id: string;
  name: string;
  system: 'amiga' | 'megadrive' | 'atarist' | 'baremetal' | 'custom';
  targetCpu: string;
  defaultOrigin: string;
  defaultOutputFormat: string;
  defaultEmulator: string;
  customExecutable?: string;
  customRomPath?: string;
  customArgs?: string;
  capabilities: PlatformCapabilities;
  memory: PlatformMemoryConfig;
  buildOptions: ProfileBuildOptions;
  templateKey?: string;
  isBuiltin?: boolean;
}

export const BUILTIN_PROFILES: Record<string, PlatformProfile> = {
  amiga500: {
    id: 'amiga500',
    name: '🕹️ Amiga 500 (OCS / 68000)',
    system: 'amiga',
    targetCpu: '68000',
    defaultOrigin: '$0000',
    defaultOutputFormat: 'adf',
    defaultEmulator: 'fsuae',
    capabilities: {
      hasCopper: true,
      hasBlitter: true,
      hasBitplanes: true,
      hasAdfFloppy: true,
      hasVdpTiles: false,
    },
    memory: {
      ramTotalKb: 512,
      ramLabel: '512 KB Chip RAM',
      romTotalKb: 512,
      romLabel: '512 KB Kickstart 1.3',
    },
    buildOptions: {
      defaultOrigin: '$0000',
      defaultOutputFormat: 'adf',
      branchRelaxation: 'auto',
      optimizeImmediates: true,
      generateMapFile: false,
    },
    templateKey: 'amiga500',
    isBuiltin: true,
  },
  amiga1200: {
    id: 'amiga1200',
    name: '🚀 Amiga 1200 (AGA / 68020)',
    system: 'amiga',
    targetCpu: '68020',
    defaultOrigin: '$0000',
    defaultOutputFormat: 'adf',
    defaultEmulator: 'fsuae',
    capabilities: {
      hasCopper: true,
      hasBlitter: true,
      hasBitplanes: true,
      hasAdfFloppy: true,
      hasVdpTiles: false,
    },
    memory: {
      ramTotalKb: 2048,
      ramLabel: '2 MB Chip RAM',
      romTotalKb: 512,
      romLabel: '512 KB Kickstart 3.1',
    },
    buildOptions: {
      defaultOrigin: '$0000',
      defaultOutputFormat: 'adf',
      branchRelaxation: 'auto',
      optimizeImmediates: true,
      generateMapFile: false,
    },
    templateKey: 'amiga500',
    isBuiltin: true,
  },
  megadrive: {
    id: 'megadrive',
    name: '🎮 Sega Mega Drive / Genesis',
    system: 'megadrive',
    targetCpu: '68000',
    defaultOrigin: '$000000',
    defaultOutputFormat: 'bin',
    defaultEmulator: 'blastem',
    capabilities: {
      hasCopper: false,
      hasBlitter: false,
      hasBitplanes: false,
      hasAdfFloppy: false,
      hasVdpTiles: true,
    },
    memory: {
      ramTotalKb: 64,
      ramLabel: '64 KB 68k RAM',
      romTotalKb: 4096,
      romLabel: '4 MB Cartridge ROM',
    },
    buildOptions: {
      defaultOrigin: '$000000',
      defaultOutputFormat: 'bin',
      branchRelaxation: 'auto',
      optimizeImmediates: true,
      generateMapFile: true,
    },
    templateKey: 'megadrive',
    isBuiltin: true,
  },
  atarist: {
    id: 'atarist',
    name: '🖥️ Atari ST (1040ST / TOS)',
    system: 'atarist',
    targetCpu: '68000',
    defaultOrigin: '$1000',
    defaultOutputFormat: 'bin',
    defaultEmulator: 'hatari',
    capabilities: {
      hasCopper: false,
      hasBlitter: true,
      hasBitplanes: true,
      hasAdfFloppy: false,
      hasVdpTiles: false,
    },
    memory: {
      ramTotalKb: 1024,
      ramLabel: '1024 KB ST RAM',
      romTotalKb: 192,
      romLabel: '192 KB TOS 1.04',
    },
    buildOptions: {
      defaultOrigin: '$1000',
      defaultOutputFormat: 'bin',
      branchRelaxation: 'auto',
      optimizeImmediates: true,
      generateMapFile: false,
    },
    templateKey: 'baremetal',
    isBuiltin: true,
  },
  baremetal: {
    id: 'baremetal',
    name: '⚡ Bare Metal 68000 System',
    system: 'baremetal',
    targetCpu: '68000',
    defaultOrigin: '$000000',
    defaultOutputFormat: 'bin',
    defaultEmulator: 'custom',
    capabilities: {
      hasCopper: false,
      hasBlitter: false,
      hasBitplanes: false,
      hasAdfFloppy: false,
      hasVdpTiles: false,
    },
    memory: {
      ramTotalKb: 64,
      ramLabel: '64 KB RAM',
      romTotalKb: 64,
      romLabel: '64 KB ROM',
    },
    buildOptions: {
      defaultOrigin: '$000000',
      defaultOutputFormat: 'bin',
      branchRelaxation: 'auto',
      optimizeImmediates: false,
      generateMapFile: true,
    },
    templateKey: 'baremetal',
    isBuiltin: true,
  },
};

const STORAGE_KEY_PROFILES = 'm68k_studio_user_profiles';

export function loadAllProfiles(): Record<string, PlatformProfile> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_PROFILES);
    if (!raw) return { ...BUILTIN_PROFILES };
    const parsed = JSON.parse(raw);
    // Ensure all built-in profiles exist and have buildOptions
    const merged: Record<string, PlatformProfile> = { ...BUILTIN_PROFILES };
    for (const [k, v] of Object.entries(parsed as Record<string, PlatformProfile>)) {
      merged[k] = {
        ...v,
        buildOptions: v.buildOptions || {
          defaultOrigin: v.defaultOrigin || '$0000',
          defaultOutputFormat: v.defaultOutputFormat || 'bin',
          branchRelaxation: 'auto',
          optimizeImmediates: true,
          generateMapFile: false,
        },
      };
    }
    return merged;
  } catch {
    return { ...BUILTIN_PROFILES };
  }
}

export function saveAllProfiles(profiles: Record<string, PlatformProfile>) {
  localStorage.setItem(STORAGE_KEY_PROFILES, JSON.stringify(profiles));
}

export function getActiveProfile(profileId?: string): PlatformProfile {
  const all = loadAllProfiles();
  if (profileId && all[profileId]) return all[profileId];
  return all.amiga500 || BUILTIN_PROFILES.amiga500;
}
