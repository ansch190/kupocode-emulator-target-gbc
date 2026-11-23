//! Die Maschine: CPU, Bus und Kassette mit den Schnittstellen aus `kupocode-emulator-core`.

use kupocode_emulator_core::audio::AudioBuffer;
use kupocode_emulator_core::debug::{
    Breakpoints, DebugTarget, DisasmLine, MemoryRegion, RegisterInfo, StackFrame,
};
use kupocode_emulator_core::input::Input;
use kupocode_emulator_core::machine::{Machine, MachineInfo, StopReason};
use kupocode_emulator_core::state::{SaveState, StateError, StateReader, StateWriter};
use kupocode_emulator_core::symbols::SymbolTable;
use kupocode_emulator_core::video::FrameBuffer;

use crate::bus::Bus;
use crate::cart::CartridgeInfo;
use crate::cpu::{Cpu, Step};
use crate::disasm;
use crate::hook::{CpuHook, HookAction, InstrCtx, NoHook};
use crate::model::Model;
use crate::ppu::{HEIGHT, WIDTH};

/// Kennung der Save-States.
pub const STATE_MAGIC: [u8; 4] = *b"KGBC";
/// Version der Save-States.
pub const STATE_VERSION: u32 = 1;

/// Graustufen des DMG (Farbindex 0–3).
const DMG_SHADES: [[u8; 3]; 4] = [[255, 255, 255], [170, 170, 170], [85, 85, 85], [0, 0, 0]];

/// Graustufen für DMG-Spiele auf dem CGB (RGB555): Weiß, Hellgrau, Dunkelgrau, Schwarz.
///
/// Das CGB-Boot-ROM wählt hier Farben nach einer Tabelle über die Titel-Prüfsumme; diese
/// Tabelle ist Teil des Boot-ROMs und nicht nachgebildet. Mit echtem Boot-ROM gilt dessen Wahl.
const COMPAT_PALETTE: [u16; 4] = [0x7FFF, 0x56B5, 0x294A, 0x0000];

/// RGB555 → RGB888 (lineare Skalierung, ohne LCD-Farbkorrektur).
pub fn rgb555(c: u16) -> [u8; 3] {
    let ch = |v: u16| {
        let v = (v & 0x1F) as u8;
        (v << 3) | (v >> 2)
    };
    [ch(c), ch(c >> 5), ch(c >> 10)]
}

/// Ergebnis eines Test-ROMs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestResult {
    Passed,
    Failed(String),
}

pub struct Gb {
    pub cpu: Cpu,
    pub bus: Bus,
    rom: Vec<u8>,
    /// Erzwungenes Modell (sonst aus dem Kopf).
    model_override: Option<Model>,
    /// Vom Anwender geladenes Boot-ROM.
    boot_rom: Option<Vec<u8>>,
    framebuffer: FrameBuffer,
    frame_done: bool,
    stopped: Option<StopReason>,
    bp_resume: Option<u16>,
    call_stack: Vec<StackFrame>,
    tty: String,
    /// Mooneye-Ergebnis nach `LD B,B`.
    mooneye: Option<TestResult>,
}

static REGISTERS: [RegisterInfo; 13] = {
    const fn r(name: &'static str, bits: u8) -> RegisterInfo {
        RegisterInfo {
            name,
            group: "SM83",
            bits,
        }
    }
    [
        r("A", 8),
        r("F", 8),
        r("B", 8),
        r("C", 8),
        r("D", 8),
        r("E", 8),
        r("H", 8),
        r("L", 8),
        r("SP", 16),
        r("PC", 16),
        r("IME", 1),
        r("IE", 8),
        r("IF", 8),
    ]
};

static REGIONS: [MemoryRegion; 10] = {
    const fn m(name: &'static str, start: u32, size: u32) -> MemoryRegion {
        MemoryRegion { name, start, size }
    }
    [
        m("ROM0", 0x0000, 0x4000),
        m("ROMX", 0x4000, 0x4000),
        m("VRAM", 0x8000, 0x2000),
        m("SRAM", 0xA000, 0x2000),
        m("WRAM0", 0xC000, 0x1000),
        m("WRAMX", 0xD000, 0x1000),
        m("OAM", 0xFE00, 0xA0),
        m("I/O", 0xFF00, 0x80),
        m("HRAM", 0xFF80, 0x7F),
        m("IE", 0xFFFF, 1),
    ]
};

impl Default for Gb {
    fn default() -> Self {
        Self::new(Model::Dmg)
    }
}

impl Gb {
    /// Maschine ohne Kassette.
    pub fn new(model: Model) -> Self {
        Self {
            cpu: Cpu::new(),
            bus: Bus::new(model, None),
            rom: Vec::new(),
            model_override: Some(model),
            boot_rom: None,
            framebuffer: FrameBuffer::new(WIDTH, HEIGHT),
            frame_done: false,
            stopped: None,
            bp_resume: None,
            call_stack: Vec::new(),
            tty: String::new(),
            mooneye: None,
        }
    }

    /// Lädt ein ROM; das Modell ergibt sich aus dem Kopf.
    pub fn from_rom(rom: &[u8]) -> Result<Self, String> {
        Self::from_rom_with(rom, None, None)
    }

    /// Lädt ein ROM mit erzwungenem Modell und/oder echtem Boot-ROM.
    pub fn from_rom_with(
        rom: &[u8],
        model: Option<Model>,
        boot_rom: Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let mut gb = Self::new(Model::Dmg);
        gb.model_override = model;
        gb.boot_rom = boot_rom;
        gb.load_program(rom)?;
        Ok(gb)
    }

    pub fn model(&self) -> Model {
        self.bus.model
    }

    /// Baut Bus und CPU neu auf und setzt den Zustand nach dem Boot (bzw. startet das Boot-ROM).
    fn power_on(&mut self) -> Result<(), String> {
        let info = CartridgeInfo::parse(&self.rom)?;
        let model = self
            .model_override
            .unwrap_or_else(|| Model::for_header(info.cgb_flag, info.sgb_flag));
        let battery = self
            .bus
            .cart
            .as_ref()
            .map(|c| c.battery_ram(0))
            .unwrap_or_default();
        let breakpoints = std::mem::take(&mut self.bus.breakpoints);
        let mut cart = info.into_mapper(self.rom.clone())?;
        if !battery.is_empty() {
            cart.load_battery_ram(&battery);
        }
        self.bus = Bus::new(model, Some(cart));
        self.bus.breakpoints = breakpoints;
        self.cpu = Cpu::new();
        self.stopped = None;
        self.call_stack.clear();
        self.tty.clear();
        self.mooneye = None;
        match &self.boot_rom {
            Some(boot) => self.bus.boot_rom = Some(boot.clone()),
            None => self.hle_boot(),
        }
        Ok(())
    }

    /// Zustand nach dem Boot-ROM (Register und I/O), ohne Nintendo-Code.
    fn hle_boot(&mut self) {
        let c = &mut self.cpu;
        let checksum_nonzero = self.rom.get(0x14D).is_some_and(|&b| b != 0);
        let (af, bc, de, hl, div) = match self.bus.model {
            Model::Dmg => (
                if checksum_nonzero { 0x01B0 } else { 0x0180 },
                0x0013,
                0x00D8,
                0x014D,
                0xABCC,
            ),
            Model::Cgb => (0x1180, 0x0000, 0xFF56, 0x000D, 0x1EA0),
            Model::Sgb => (0x0100, 0x0014, 0x0000, 0xC060, 0x0000),
            Model::Sgb2 => (0xFF00, 0x0014, 0x0000, 0xC060, 0x0000),
        };
        c.set_af(af);
        c.set_bc(bc);
        c.set_de(de);
        c.set_hl(hl);
        c.sp = 0xFFFE;
        c.pc = 0x0100;
        let b = &mut self.bus;
        b.timer.counter = div;
        b.joypad.select = 0x30;
        b.if_ = 0x01;
        const APU: [(u16, u8); 16] = [
            (0xFF10, 0x80),
            (0xFF11, 0xBF),
            (0xFF12, 0xF3),
            (0xFF13, 0xFF),
            (0xFF14, 0xBF),
            (0xFF16, 0x3F),
            (0xFF18, 0xFF),
            (0xFF19, 0xBF),
            (0xFF1A, 0x7F),
            (0xFF1B, 0xFF),
            (0xFF1C, 0x9F),
            (0xFF1E, 0xBF),
            (0xFF20, 0xFF),
            (0xFF23, 0xBF),
            (0xFF24, 0x77),
            (0xFF25, 0xF3),
        ];
        b.poke(0xFF26, 0x80);
        for (addr, v) in APU {
            b.poke(addr, v);
        }
        b.poke(0xFF26, if b.model.is_sgb() { 0xF0 } else { 0xF1 });
        if b.model.is_cgb() && self.rom.get(0x143).is_some_and(|&f| f & 0x80 == 0) {
            // DMG-Spiel auf dem CGB: Kompatibilitätsmodus mit Graustufen-Paletten.
            let c = &mut b.ppu.color;
            c.cgb_mode = false;
            for (i, col) in COMPAT_PALETTE.iter().enumerate() {
                let [lo, hi] = col.to_le_bytes();
                // BG-Palette 0, Objektpaletten 0 und 1
                for (pal, base) in [(0, 0), (1, 0), (1, 8)] {
                    let target = if pal == 0 {
                        &mut c.bg_pal
                    } else {
                        &mut c.obj_pal
                    };
                    target[base + i * 2] = lo;
                    target[base + i * 2 + 1] = hi;
                }
            }
        }
        b.poke(0xFF40, 0x91);
        b.poke(0xFF47, 0xFC);
        b.dma_reg = if b.model.is_cgb() { 0x00 } else { 0xFF };
    }

    /// Adresse im Code mit Bank: `(bank << 16) | addr` für umschaltbare Bereiche.
    pub fn code_address(&self, pc: u16) -> u32 {
        let bank = self.bus.cart.as_ref().map_or(0, |c| c.bank_at(pc));
        if (0x4000..0x8000).contains(&pc) && bank > 1 {
            ((bank as u32) << 16) | u32::from(pc)
        } else {
            u32::from(pc)
        }
    }

    /// Führt einen Schritt aus; `Some` bei einem Halt.
    pub fn step_with(&mut self, hook: &mut impl CpuHook) -> Option<StopReason> {
        if let Some(stop) = &self.stopped {
            return Some(stop.clone());
        }
        if self.bus.cart.is_none() {
            return Some(StopReason::Error {
                pc: 0,
                msg: "keine Kassette eingelegt".into(),
            });
        }
        let pc = self.cpu.pc;
        if self.bus.breakpoints.has_exec() && self.bp_resume != Some(pc) {
            let addr = self.code_address(pc);
            if self.bus.breakpoints.hits(addr) {
                return Some(StopReason::Breakpoint { addr });
            }
        }
        self.bp_resume = None;
        let opcode = self.bus.peek(pc);
        let ctx = InstrCtx {
            pc,
            opcode,
            af: self.cpu.af(),
            bc: self.cpu.bc(),
            de: self.cpu.de(),
            hl: self.cpu.hl(),
            sp: self.cpu.sp,
            cycle: self.bus.cycles,
        };
        if let HookAction::Stop(msg) = hook.before_execute(&ctx) {
            let stop = StopReason::Error {
                pc: u32::from(pc),
                msg,
            };
            self.stopped = Some(stop.clone());
            return Some(stop);
        }
        let sp_before = self.cpu.sp;
        let step = self.cpu.step(&mut self.bus);
        self.track_calls(step, sp_before);
        if self.bus.ppu.take_frame() {
            self.frame_done = true;
            self.update_framebuffer();
        }
        match step {
            Step::Locked { opcode, pc } => {
                let stop = StopReason::Error {
                    pc: u32::from(pc),
                    msg: format!("CPU hängt (gesperrter Opcode ${opcode:02X})"),
                };
                self.stopped = Some(stop.clone());
                return Some(stop);
            }
            Step::Instruction { opcode: 0x40, .. } => self.check_mooneye(),
            _ => {}
        }
        if let Some((addr, access)) = self.bus.watch_hit.take() {
            return Some(StopReason::Watchpoint {
                addr,
                access,
                pc: u32::from(pc),
            });
        }
        None
    }

    fn check_mooneye(&mut self) {
        let c = &self.cpu;
        let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
        if regs == [3, 5, 8, 13, 21, 34] {
            self.mooneye = Some(TestResult::Passed);
        } else if regs == [0x42; 6] {
            self.mooneye = Some(TestResult::Failed("Mooneye: Fehlerregister 0x42".into()));
        }
    }

    fn track_calls(&mut self, step: Step, sp_before: u16) {
        match step {
            Step::Instruction { opcode, pc }
                if matches!(opcode, 0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC)
                    || (opcode & 0xC7 == 0xC7) =>
            {
                if self.cpu.sp == sp_before.wrapping_sub(2) {
                    let len = if opcode & 0xC7 == 0xC7 { 1 } else { 3 };
                    let ret = pc.wrapping_add(len);
                    self.push_frame(ret);
                }
            }
            Step::Interrupt { return_pc, .. } => self.push_frame(return_pc),
            Step::Instruction { opcode, .. }
                if matches!(opcode, 0xC9 | 0xD9 | 0xC0 | 0xC8 | 0xD0 | 0xD8)
                    && self.cpu.sp == sp_before.wrapping_add(2) =>
            {
                let pc = self.code_address(self.cpu.pc);
                if let Some(i) = self.call_stack.iter().rposition(|f| f.return_addr == pc) {
                    self.call_stack.truncate(i);
                } else {
                    self.call_stack.pop();
                }
            }
            _ => {}
        }
    }

    fn push_frame(&mut self, return_pc: u16) {
        let frame = StackFrame {
            function: self.code_address(self.cpu.pc),
            return_addr: self.code_address(return_pc),
            sp: u32::from(self.cpu.sp),
        };
        self.call_stack.push(frame);
    }

    fn update_framebuffer(&mut self) {
        if let Some(sgb) = &mut self.bus.sgb {
            sgb.end_frame(&self.bus.ppu.lines);
            sgb.compose(&self.bus.ppu.lines, &mut self.framebuffer);
            return;
        }
        let cgb = self.bus.ppu.color.cgb;
        for (i, &px) in self.bus.ppu.lines.iter().enumerate() {
            let x = i as u32 % WIDTH;
            let y = i as u32 / WIDTH;
            let rgb = if cgb {
                rgb555(px)
            } else {
                DMG_SHADES[usize::from(px & 3)]
            };
            self.framebuffer.set_pixel(x, y, rgb);
        }
    }

    fn refresh_tty(&mut self) {
        if self.tty.len() != self.bus.serial.output.len() {
            self.tty = self.bus.serial.output.iter().map(|&b| b as char).collect();
        }
    }

    /// Hängt das Programm erkennbar fest (`JR -2` ohne Unterbrechungen, `HALT` ohne `IE`)?
    fn is_halted(&self) -> bool {
        let pc = self.cpu.pc;
        let no_irq = (!self.cpu.ime && self.cpu.ei_delay == 0) || self.bus.ie & 0x1F == 0;
        let self_loop = self.bus.peek(pc) == 0x18 && self.bus.peek(pc.wrapping_add(1)) == 0xFE;
        (self_loop && no_irq) || (self.cpu.halted && self.bus.ie & 0x1F == 0)
    }

    /// Ein Schritt ohne Hook (für den Linkbetrieb).
    pub fn link_step(&mut self) -> Option<StopReason> {
        let stop = self.step_with(&mut NoHook);
        self.refresh_tty();
        stop
    }

    /// Ist seit der letzten Abfrage ein Bild fertig geworden?
    pub fn take_frame_done(&mut self) -> bool {
        std::mem::take(&mut self.frame_done)
    }

    pub fn run_frame_with(&mut self, hook: &mut impl CpuHook) -> StopReason {
        self.frame_done = false;
        self.bp_resume = Some(self.cpu.pc);
        let stop = loop {
            if let Some(s) = self.step_with(hook) {
                break s;
            }
            if self.frame_done {
                break StopReason::FrameDone;
            }
        };
        self.refresh_tty();
        stop
    }

    pub fn run_steps(&mut self, max_steps: u64, hook: &mut impl CpuHook) -> StopReason {
        let mut stop = StopReason::StepLimit;
        for _ in 0..max_steps {
            if let Some(s) = self.step_with(hook) {
                stop = s;
                break;
            }
            if self.is_halted() {
                stop = StopReason::Halted;
                break;
            }
        }
        self.refresh_tty();
        stop
    }

    /// Läuft, bis ein Test-ROM ein Ergebnis meldet (Blargg über die serielle Ausgabe, Mooneye über
    /// `LD B,B`) oder `max_frames` Bilder vergangen sind.
    pub fn run_test(&mut self, max_frames: u32) -> Option<TestResult> {
        for _ in 0..max_frames {
            let stop = self.run_frame_with(&mut NoHook);
            if let Some(r) = self.mooneye.clone() {
                return Some(r);
            }
            if self.tty.contains("Passed") {
                return Some(TestResult::Passed);
            }
            if self.tty.contains("Failed") {
                return Some(TestResult::Failed(self.tty.clone()));
            }
            if stop != StopReason::FrameDone {
                return Some(TestResult::Failed(stop.to_string()));
            }
        }
        None
    }

    fn step_until(&mut self, done: impl Fn(&Gb) -> bool, limit: u64) -> StopReason {
        self.bp_resume = Some(self.cpu.pc);
        for _ in 0..limit {
            if let Some(stop) = self.step_with(&mut NoHook) {
                return stop;
            }
            if done(self) {
                self.refresh_tty();
                return StopReason::StepDone;
            }
        }
        StopReason::StepLimit
    }

    /// Batteriegepuffertes RAM samt Uhr; `now` ist der Unix-Zeitstempel für den Uhr-Anhang.
    /// Leer, wenn die Kassette keine Batterie hat.
    pub fn battery_ram(&self, now: u64) -> Vec<u8> {
        self.bus
            .cart
            .as_ref()
            .map(|c| c.battery_ram(now))
            .unwrap_or_default()
    }

    /// Lädt das Batterie-RAM; liefert den gespeicherten Zeitstempel der Uhr, falls vorhanden.
    pub fn load_battery_ram(&mut self, data: &[u8]) -> Option<u64> {
        self.bus.cart.as_mut()?.load_battery_ram(data)
    }

    /// Stellt die Uhr der Kassette um `seconds` vor.
    pub fn advance_rtc(&mut self, seconds: u64) {
        if let Some(c) = self.bus.cart.as_mut() {
            c.advance_rtc(seconds);
        }
    }

    /// Setzt das Sensorbild der Pocket Camera (128 × 112 Graustufen, 0 = schwarz).
    pub fn set_camera_image(&mut self, image: &[u8]) {
        if let Some(c) = self.bus.cart.as_mut() {
            c.set_camera_image(image);
        }
    }

    /// Läuft der Rumble-Motor der Kassette?
    pub fn rumble(&self) -> bool {
        self.bus.cart.as_ref().is_some_and(|c| c.rumble())
    }

    pub fn save_state_bytes(&self) -> Vec<u8> {
        let mut w = StateWriter::new(STATE_MAGIC, STATE_VERSION);
        self.save_state(&mut w);
        w.finish()
    }

    pub fn load_state_bytes(&mut self, data: &[u8]) -> Result<(), StateError> {
        let mut r = StateReader::new(data, STATE_MAGIC, STATE_VERSION)?;
        self.load_state(&mut r)
    }
}

impl Machine for Gb {
    fn info(&self) -> MachineInfo {
        MachineInfo {
            platform: "gb",
            name: self.bus.model.name().into(),
            cpu_hz: self.bus.model.cpu_hz(),
            frame_rate_mhz: 59_727,
        }
    }

    fn reset(&mut self) {
        if !self.rom.is_empty() {
            // Das ROM wurde beim Laden geprüft; ein Fehler ist hier nicht möglich.
            let _ = self.power_on();
        }
    }

    fn load_program(&mut self, bytes: &[u8]) -> Result<(), String> {
        CartridgeInfo::parse(bytes)?;
        self.rom = bytes.to_vec();
        self.bus.cart = None;
        self.power_on()
    }

    fn set_input(&mut self, input: &Input) {
        for (i, pad) in input.pads.iter().enumerate() {
            let buttons = if pad.connected {
                pad.buttons
            } else {
                Default::default()
            };
            self.bus.joypad.set_buttons(i, buttons);
        }
        if let Some(c) = self.bus.cart.as_mut() {
            let (x, y) = input.pads[0].left_stick;
            c.set_tilt(x, y);
        }
    }

    fn run_frame(&mut self) -> StopReason {
        self.run_frame_with(&mut NoHook)
    }

    fn run(&mut self, max_steps: u64) -> StopReason {
        self.run_steps(max_steps, &mut NoHook)
    }

    fn framebuffer(&self) -> &FrameBuffer {
        &self.framebuffer
    }

    fn drain_audio(&mut self, out: &mut AudioBuffer) {
        for pair in self.bus.apu.take_samples().as_chunks::<2>().0 {
            out.push(pair[0], pair[1]);
        }
    }

    fn cycles(&self) -> u64 {
        self.bus.cycles
    }

    fn tty_output(&self) -> &str {
        &self.tty
    }
}

impl DebugTarget for Gb {
    fn registers(&self) -> &'static [RegisterInfo] {
        &REGISTERS
    }

    fn read_register(&self, id: usize) -> u64 {
        let c = &self.cpu;
        u64::from(match id {
            0 => u16::from(c.a),
            1 => u16::from(c.f),
            2 => u16::from(c.b),
            3 => u16::from(c.c),
            4 => u16::from(c.d),
            5 => u16::from(c.e),
            6 => u16::from(c.h),
            7 => u16::from(c.l),
            8 => c.sp,
            9 => c.pc,
            10 => u16::from(c.ime),
            11 => u16::from(self.bus.ie),
            12 => u16::from(self.bus.if_),
            _ => 0,
        })
    }

    fn write_register(&mut self, id: usize, value: u64) -> bool {
        let b = value as u8;
        let c = &mut self.cpu;
        match id {
            0 => c.a = b,
            1 => c.f = b & 0xF0,
            2 => c.b = b,
            3 => c.c = b,
            4 => c.d = b,
            5 => c.e = b,
            6 => c.h = b,
            7 => c.l = b,
            8 => c.sp = value as u16,
            9 => c.pc = value as u16,
            10 => c.ime = value != 0,
            11 => self.bus.ie = b,
            12 => self.bus.if_ = b & 0x1F,
            _ => return false,
        }
        true
    }

    fn graphics_views(&self) -> Vec<kupocode_emulator_core::debug::GraphicsView> {
        crate::views::views(&self.bus.ppu, self.bus.model.is_cgb())
    }

    fn pc(&self) -> u32 {
        self.code_address(self.cpu.pc)
    }

    fn memory_regions(&self) -> &'static [MemoryRegion] {
        &REGIONS
    }

    fn peek(&self, addr: u32, buf: &mut [u8]) {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.bus.peek((addr as u16).wrapping_add(i as u16));
        }
    }

    fn poke(&mut self, addr: u32, data: &[u8]) -> Result<(), String> {
        for (i, &v) in data.iter().enumerate() {
            let a = (addr as u16).wrapping_add(i as u16);
            if a < 0x8000 {
                return Err(format!("Adresse ${a:04X} liegt im ROM"));
            }
            self.bus.poke(a, v);
        }
        Ok(())
    }

    fn disassemble(
        &self,
        addr: u32,
        count: usize,
        symbols: Option<&SymbolTable>,
    ) -> Vec<DisasmLine> {
        disasm::lines(addr as u16, count, |a| self.bus.peek(a))
            .into_iter()
            .map(|(a, bytes, text)| {
                let target = disasm::target(&bytes, a).map(|t| self.code_address(t));
                let label = symbols.and_then(|s| s.name_at(self.code_address(a)).map(String::from));
                DisasmLine {
                    addr: u32::from(a),
                    bytes,
                    text,
                    target,
                    label,
                }
            })
            .collect()
    }

    fn call_stack(&self) -> Vec<StackFrame> {
        self.call_stack.iter().rev().cloned().collect()
    }

    fn breakpoints(&self) -> &Breakpoints {
        &self.bus.breakpoints
    }

    fn breakpoints_mut(&mut self) -> &mut Breakpoints {
        &mut self.bus.breakpoints
    }

    fn step_into(&mut self) -> StopReason {
        self.step_until(|_| true, 1)
    }

    fn step_over(&mut self) -> StopReason {
        let pc = self.cpu.pc;
        let op = self.bus.peek(pc);
        let len = match op {
            0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC => 3,
            _ if op & 0xC7 == 0xC7 => 1,
            _ => return self.step_into(),
        };
        let depth = self.call_stack.len();
        let ret = pc.wrapping_add(len);
        self.step_until(
            move |g| g.cpu.pc == ret && g.call_stack.len() <= depth,
            50_000_000,
        )
    }

    fn step_out(&mut self) -> StopReason {
        let depth = self.call_stack.len();
        if depth == 0 {
            return self.step_into();
        }
        self.step_until(move |g| g.call_stack.len() < depth, 50_000_000)
    }
}

impl SaveState for Gb {
    fn save_state(&self, w: &mut StateWriter) {
        w.u8(self.bus.model.id());
        let c = &self.cpu;
        for v in [c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l, c.ei_delay] {
            w.u8(v);
        }
        w.u16(c.sp);
        w.u16(c.pc);
        for v in [c.ime, c.halted, c.halt_bug, c.locked] {
            w.bool(v);
        }
        self.bus.save_state(w);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        let model = Model::from_id(r.u8()?).ok_or(StateError::Invalid("Modell".into()))?;
        if model != self.bus.model {
            return Err(StateError::Invalid(format!(
                "Save-State gehört zu {}, geladen ist {}",
                model.name(),
                self.bus.model.name()
            )));
        }
        let c = &mut self.cpu;
        for v in [
            &mut c.a,
            &mut c.f,
            &mut c.b,
            &mut c.c,
            &mut c.d,
            &mut c.e,
            &mut c.h,
            &mut c.l,
            &mut c.ei_delay,
        ] {
            *v = r.u8()?;
        }
        c.sp = r.u16()?;
        c.pc = r.u16()?;
        for v in [&mut c.ime, &mut c.halted, &mut c.halt_bug, &mut c.locked] {
            *v = r.bool()?;
        }
        self.bus.boot_rom = self.boot_rom.clone();
        self.bus.load_state(r)?;
        self.stopped = None;
        self.call_stack.clear();
        self.refresh_tty();
        self.update_framebuffer();
        Ok(())
    }
}
