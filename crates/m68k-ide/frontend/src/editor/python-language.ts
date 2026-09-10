//! Python Language Support & m68k Hardware Intellisense for Monaco Editor.

import * as monaco from 'monaco-editor';

export function registerM68kPythonLanguage() {
  // Register custom Completion Item Provider for Python targeting m68k hardware
  monaco.languages.registerCompletionItemProvider('python', {
    triggerCharacters: ['.', '(', ' '],
    provideCompletionItems: (model, position) => {
      const lineContent = model.getLineContent(position.lineNumber);
      const textUntilPosition = lineContent.substring(0, position.column - 1);

      const suggestions: monaco.languages.CompletionItem[] = [];

      // Check if user is typing "custom." (Amiga Custom Hardware)
      if (textUntilPosition.endsWith('custom.')) {
        const amigaRegs = [
          { name: 'color00', doc: 'Background Color register ($DFF180, 12-bit RGB $0RGB)' },
          { name: 'color01', doc: 'Palette Color 1 register ($DFF182, 12-bit RGB)' },
          { name: 'color02', doc: 'Palette Color 2 register ($DFF184, 12-bit RGB)' },
          { name: 'color03', doc: 'Palette Color 3 register ($DFF186, 12-bit RGB)' },
          { name: 'dmacon', doc: 'DMA Control register ($DFF096 - Set/Clear DMA channels)' },
          { name: 'intena', doc: 'Interrupt Enable register ($DFF09A - Master/VBL/Copper/Blitter IRQ)' },
          { name: 'intreq', doc: 'Interrupt Request register ($DFF09C)' },
          { name: 'bplcon0', doc: 'Bitplane Control 0 ($DFF100 - Number of bitplanes, color burst, composite)' },
          { name: 'bplcon1', doc: 'Bitplane Control 1 ($DFF102 - Horizontal scroll offsets)' },
          { name: 'cop1lch', doc: 'Copper 1 Location High ($DFF080 - 32-bit pointer to Copperlist)' },
          { name: 'copjmp1', doc: 'Copper 1 Restart Strobe ($DFF088 - Jumps to Cop1Lch)' },
          { name: 'bltcon0', doc: 'Blitter Control 0 ($DFF040 - Minterm, shift A, channels ABCD)' },
          { name: 'bltcon1', doc: 'Blitter Control 1 ($DFF042 - Shift B, line mode, fill/descending)' },
          { name: 'bltsize', doc: 'Blitter Size ($DFF058 - Trigger Blit: H lines x W words)' },
          { name: 'bltapt', doc: 'Blitter Source A pointer ($DFF050)' },
          { name: 'bltdpt', doc: 'Blitter Destination D pointer ($DFF054)' },
        ];

        for (const reg of amigaRegs) {
          suggestions.push({
            label: reg.name,
            kind: monaco.languages.CompletionItemKind.Property,
            documentation: reg.doc,
            insertText: reg.name,
            range: {
              startLineNumber: position.lineNumber,
              startColumn: position.column,
              endLineNumber: position.lineNumber,
              endColumn: position.column,
            },
          });
        }
        return { suggestions };
      }

      // Check if user is typing "vdp." (Sega Mega Drive VDP)
      if (textUntilPosition.endsWith('vdp.')) {
        const vdpRegs = [
          { name: 'control', doc: 'VDP Control Port ($C00004 - Write register / Set VRAM/CRAM address)' },
          { name: 'data', doc: 'VDP Data Port ($C00000 - Write tile data / color values)' },
          { name: 'backdrop', doc: 'Backdrop Color register ($87xx command)' },
          { name: 'color', doc: 'Set CRAM color index and RGB value' },
        ];

        for (const reg of vdpRegs) {
          suggestions.push({
            label: reg.name,
            kind: monaco.languages.CompletionItemKind.Property,
            documentation: reg.doc,
            insertText: reg.name,
            range: {
              startLineNumber: position.lineNumber,
              startColumn: position.column,
              endLineNumber: position.lineNumber,
              endColumn: position.column,
            },
          });
        }
        return { suggestions };
      }

      // Built-in m68k Python functions & hardware objects
      const builtins = [
        {
          label: 'from m68k.amiga import custom, wait_vbl, wait_line',
          kind: monaco.languages.CompletionItemKind.Snippet,
          documentation: 'Import Amiga 500 custom chip hardware and timing helpers.',
          insertText: 'from m68k.amiga import custom, wait_vbl, wait_line',
        },
        {
          label: 'from m68k.megadrive import vdp',
          kind: monaco.languages.CompletionItemKind.Snippet,
          documentation: 'Import Sega Mega Drive VDP hardware registers.',
          insertText: 'from m68k.megadrive import vdp',
        },
        {
          label: 'wait_vbl()',
          kind: monaco.languages.CompletionItemKind.Function,
          documentation: 'Wait for Vertical Blanking Interval (Beam pos $130 / VBL).',
          insertText: 'wait_vbl()',
        },
        {
          label: 'wait_line(line)',
          kind: monaco.languages.CompletionItemKind.Function,
          documentation: 'Wait for specific PAL/NTSC raster scanline (0-312).',
          insertText: 'wait_line(${1:128})',
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        },
        {
          label: 'poke_w(addr, val)',
          kind: monaco.languages.CompletionItemKind.Function,
          documentation: 'Write a 16-bit word directly to memory address (move.w val,(addr)).',
          insertText: 'poke_w(${1:0xDFF180}, ${2:0x0F80})',
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        },
        {
          label: 'poke_l(addr, val)',
          kind: monaco.languages.CompletionItemKind.Function,
          documentation: 'Write a 32-bit longword directly to memory address (move.l val,(addr)).',
          insertText: 'poke_l(${1:0xDFF080}, ${2:0x00020000})',
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        },
        {
          label: 'asm("instruction")',
          kind: monaco.languages.CompletionItemKind.Function,
          documentation: 'Insert inline Motorola 68000 assembly instructions.',
          insertText: 'asm("${1:move.w #$8280,$DFF096}")',
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        },
        {
          label: 'def main():',
          kind: monaco.languages.CompletionItemKind.Snippet,
          documentation: 'Main entry point subroutine for m68k Python program.',
          insertText: 'def main():\n    ${1:# Your code here}\n',
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        },
        {
          label: 'while True:',
          kind: monaco.languages.CompletionItemKind.Snippet,
          documentation: 'Infinite game/demo loop.',
          insertText: 'while True:\n    ${1:wait_vbl()}\n',
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
        },
      ];

      for (const item of builtins) {
        suggestions.push({
          label: item.label,
          kind: item.kind,
          documentation: item.documentation,
          insertText: item.insertText,
          insertTextRules: item.insertTextRules,
          range: {
            startLineNumber: position.lineNumber,
            startColumn: position.column,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          },
        });
      }

      return { suggestions };
    },
  });

  // Register Hover Provider for Python m68k hardware docs
  monaco.languages.registerHoverProvider('python', {
    provideHover: (model, position) => {
      const word = model.getWordAtPosition(position);
      if (!word) return null;

      const docMap: Record<string, string> = {
        custom: '**Amiga Custom Hardware Base (`$DFF000`)**\n\nProvides access to OCS/ECS/AGA registers (Copper, Blitter, Bitplanes, Audio, CIA).',
        vdp: '**Sega Mega Drive VDP Base (`$C00000` / `$C00004`)**\n\nDirect control of Video Display Processor, VRAM, CRAM, and VSRAM.',
        color00: '**`custom.color00` (Address: `$DFF180`)**\n\nAmiga background / palette entry 0. 12-bit RGB (`$0RGB`).',
        dmacon: '**`custom.dmacon` (Address: `$DFF096`)**\n\nDirect Memory Access Control. Bit 15 determines SET/CLEAR. `$7FFF` disables all channels; `$8280` enables DMA+Copper.',
        intena: '**`custom.intena` (Address: `$DFF09A`)**\n\nInterrupt Enable register. Controls Master, VBL, Copper, and Blitter interrupts.',
        wait_vbl: '**`wait_vbl()`**\n\nWaits for the vertical blank beam position (Start of new video frame, 50Hz PAL / 60Hz NTSC).',
        wait_line: '**`wait_line(line_number)`**\n\nSynchronizes CPU execution with raster line scan position for flicker-free copper bars and palette splits.',
        poke_w: '**`poke_w(address, value)`**\n\nWrites a 16-bit word to the specified memory or hardware address (`move.w value,(address)`).',
        poke_l: '**`poke_l(address, value)`**\n\nWrites a 32-bit longword to the specified memory or hardware address (`move.l value,(address)`).',
        asm: '**`asm("instruction")`**\n\nInjects raw Motorola 68000 assembly directly into the generated machine code stream.',
      };

      const doc = docMap[word.word];
      if (doc) {
        return {
          contents: [{ value: doc }],
        };
      }
      return null;
    },
  });
}
