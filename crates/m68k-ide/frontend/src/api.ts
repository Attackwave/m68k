//! API client for communicating with the m68k backend.

export interface AssembleRequest {
  source: string;
  cpu?: string;
  base_address?: number;
  platform?: string;
}

export interface AssembleErrorItem {
  line: number;
  message: string;
}

export interface AssembleResponse {
  success: boolean;
  byte_count: number;
  errors: AssembleErrorItem[];
  warnings: string[];
  binary_base64?: string;
  hex_dump?: string;
  generated_asm?: string;
}

export interface DisassembleLineItem {
  address: number;
  address_hex: string;
  instruction_hex: string;
  text: string;
  comment?: string;
}

export interface DisassembleResponse {
  lines: DisassembleLineItem[];
}

export interface AdfEntryItem {
  name: string;
  is_dir: boolean;
  size: number;
  block: number;
}

export interface AdfInfoResponse {
  volume_name: string;
  is_ffs: boolean;
  free_blocks: number;
  total_blocks: number;
  entries: AdfEntryItem[];
}

export interface BitplaneConvertResponse {
  width: number;
  height: number;
  bitplanes_used: number;
  palette_hex: string[];
  copper_palette_asm: string;
  bitplane_asm_dc: string;
  total_bytes: number;
}

export interface CopperInstructionItem {
  op_type: string;
  word1: number;
  word2: number;
  description: string;
  vpos?: number;
  hpos?: number;
  reg_name?: string;
  color_preview_hex?: string;
}

export interface ParseCopperResponse {
  instructions: CopperInstructionItem[];
}

export interface ProjectConfig {
  name: string;
  target_cpu: string;
  main_file: string;
  output_format: string;
  origin_address: string;
  emulator_profile: string;
  include_dirs: string[];
}

export interface EmulatorProfile {
  id: string;
  name: string;
  executable_path: string;
  default_args: string[];
  supported_formats: string[];
  available: boolean;
}

export interface LaunchEmulatorFullRequest {
  emulator_type: string;
  target_file_path?: string;
  target_file_name?: string;
  target_file_data_base64?: string;
  custom_executable?: string;
  kickstart_rom_path?: string;
  extra_args?: string[];
}

export interface IdeDiagnosticItem {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
  message: string;
  severity: number;
}

export interface IdeCompletionItem {
  label: string;
  kind: string;
  detail?: string;
  documentation?: string;
  insert_text?: string;
}

export interface IdeHoverResponse {
  contents: string;
}

export interface IdeSymbolItem {
  name: string;
  kind: string;
  line: number;
  character: number;
}

const API_BASE = '/api';

export const api = {
  async assemble(req: AssembleRequest): Promise<AssembleResponse> {
    try {
      const res = await fetch(`${API_BASE}/assemble`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req),
      });
      return await res.json();
    } catch (e: any) {
      return {
        success: false,
        byte_count: 0,
        errors: [{ line: 0, message: `Failed to assemble: ${e.message || e}` }],
        warnings: [],
      };
    }
  },

  async disassemble(bytes: number[], origin: number = 0, cpu: string = '68000'): Promise<DisassembleResponse> {
    try {
      const res = await fetch(`${API_BASE}/disassemble`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ bytes, origin, cpu }),
      });
      return await res.json();
    } catch {
      return { lines: [] };
    }
  },

  async createAdf(disk_name: string, is_ffs: boolean, boot_code?: number[]): Promise<number[]> {
    try {
      const res = await fetch(`${API_BASE}/floppy/create`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ disk_name, is_ffs, boot_code }),
      });
      return await res.json();
    } catch {
      return [];
    }
  },

  async inspectAdf(adf_bytes: number[]): Promise<AdfInfoResponse | null> {
    try {
      const res = await fetch(`${API_BASE}/floppy/inspect`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(adf_bytes),
      });
      return await res.json();
    } catch {
      return null;
    }
  },

  async convertBitplanes(image_bytes: number[], max_bitplanes: number = 5, interleaved: boolean = false): Promise<BitplaneConvertResponse | null> {
    try {
      const res = await fetch(`${API_BASE}/bitplane/convert`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ image_bytes, max_bitplanes, interleaved }),
      });
      return await res.json();
    } catch {
      return null;
    }
  },

  async parseCopper(bytes: number[]): Promise<ParseCopperResponse> {
    try {
      const res = await fetch(`${API_BASE}/copper/parse`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(bytes),
      });
      return await res.json();
    } catch {
      return { instructions: [] };
    }
  },

  async scaffoldProject(target_directory: string, template: string, project_name: string): Promise<ProjectConfig | null> {
    try {
      const res = await fetch(`${API_BASE}/project/scaffold`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target_directory, template, project_name }),
      });
      return await res.json();
    } catch {
      return null;
    }
  },

  async detectEmulators(): Promise<EmulatorProfile[]> {
    try {
      const res = await fetch(`${API_BASE}/emulator/detect`);
      return await res.json();
    } catch {
      return [];
    }
  },

  async launchEmulator(req: LaunchEmulatorFullRequest): Promise<{ success: boolean; message: string }> {
    try {
      const res = await fetch(`${API_BASE}/emulator/launch`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(req),
      });
      const data = await res.json();
      if (data.error) {
        return { success: false, message: data.error };
      }
      return data;
    } catch (err: any) {
      return {
        success: false,
        message: `Could not launch emulator (${req.emulator_type}): ${err.message || err}`,
      };
    }
  },

  async installEmulator(emulator_id: string): Promise<{ success: boolean; executable_path?: string; message: string }> {
    try {
      const res = await fetch(`${API_BASE}/emulator/install`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ emulator_id }),
      });
      return await res.json();
    } catch (err: any) {
      return { success: false, message: `Fehler beim Download: ${err.message || err}` };
    }
  },

  async transpilePython(
    python_code: string,
    target_platform?: string,
    target_cpu?: string
  ): Promise<{ success: boolean; asm_code: string; errors: string[]; warnings: string[] }> {
    try {
      const res = await fetch(`${API_BASE}/python/transpile`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ python_code, target_platform, target_cpu }),
      });
      return await res.json();
    } catch (err: any) {
      return { success: false, asm_code: '', errors: [err.message || String(err)], warnings: [] };
    }
  },

  // LSP Bridge queries
  async lspHover(source: string, line: number, character: number): Promise<IdeHoverResponse | null> {
    try {
      const res = await fetch(`${API_BASE}/lsp/hover`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source, line, character }),
      });
      return await res.json();
    } catch {
      return null;
    }
  },

  async lspCompletion(source: string, line: number, character: number): Promise<IdeCompletionItem[]> {
    try {
      const res = await fetch(`${API_BASE}/lsp/completion`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source, line, character }),
      });
      return await res.json();
    } catch {
      return [];
    }
  },

  async lspDiagnostics(source: string, cpu: string = '68000'): Promise<IdeDiagnosticItem[]> {
    try {
      const res = await fetch(`${API_BASE}/lsp/diagnostics`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify([source, cpu]),
      });
      return await res.json();
    } catch {
      return [];
    }
  },

  async lspFormat(source: string, tab_size: number = 4): Promise<string> {
    try {
      const res = await fetch(`${API_BASE}/lsp/format`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify([source, tab_size]),
      });
      return await res.json();
    } catch {
      return source;
    }
  },

  async lspSymbols(source: string): Promise<IdeSymbolItem[]> {
    try {
      const res = await fetch(`${API_BASE}/lsp/symbols`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(source),
      });
      return await res.json();
    } catch {
      return [];
    }
  },

  async lspDefinition(source: string, line: number, character: number): Promise<{ line: number; character: number } | null> {
    try {
      const res = await fetch(`${API_BASE}/lsp/definition`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source, line, character }),
      });
      return await res.json();
    } catch {
      return null;
    }
  },

  async analyzeCycles(source: string, cpu: string = '68000'): Promise<CycleAnalysisResponse> {
    try {
      const res = await fetch(`${API_BASE}/analysis/cycles`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source, cpu }),
      });
      return await res.json();
    } catch {
      return {
        lines: [],
        total_min_cycles: 0,
        total_max_cycles: 0,
        pal_scanlines: 0,
        frame_percent_pal: 0,
      };
    }
  },

  async getHardwareRegisters(): Promise<HardwareRegisterInfo[]> {
    try {
      const res = await fetch(`${API_BASE}/hardware/registers`);
      return await res.json();
    } catch {
      return [];
    }
  },
};

export interface LineCycleItem {
  line: number;
  mnemonic: string;
  min_cycles: number;
  max_cycles: number;
  comment: string;
}

export interface CycleAnalysisResponse {
  lines: LineCycleItem[];
  total_min_cycles: number;
  total_max_cycles: number;
  pal_scanlines: number;
  frame_percent_pal: number;
}

export interface RegisterBitField {
  bit_range: string;
  name: string;
  description: string;
}

export interface HardwareRegisterInfo {
  system: string;
  name: string;
  address_hex: string;
  address_num: number;
  read_write: string;
  description: string;
  bitfields: RegisterBitField[];
}
