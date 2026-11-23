//! Linkkabel: zwei Geräte im Gleichtakt in einem Prozess.
//!
//! [`GbLink`] führt immer das Gerät aus, das zeitlich zurückliegt (nach `Bus::cycles` im
//! Grundtakt), jeweils einen Schritt. Vor dem Schritt sieht es das oberste Bit von `SB` der
//! Gegenstelle; danach gehen seine Taktimpulse an die Gegenstelle. Ein Bit dauert mindestens
//! 16 Takte, ein Schritt höchstens 24 – die Verschiebung bleibt also unter einem Bit.
//!
//! Das Bild zeigt beide Geräte nebeneinander; der Debugger wählt das Gerät über `select_cpu`.
//! Pad 1 steuert Gerät 1, Pad 2 Gerät 2.

use kupocode_emulator_core::audio::AudioBuffer;
use kupocode_emulator_core::debug::{
    Breakpoints, DebugTarget, DisasmLine, GraphicsView, MemoryRegion, RegisterInfo, StackFrame,
};
use kupocode_emulator_core::input::Input;
use kupocode_emulator_core::machine::{Machine, MachineInfo, StopReason};
use kupocode_emulator_core::state::{SaveState, StateError, StateReader, StateWriter};
use kupocode_emulator_core::symbols::SymbolTable;
use kupocode_emulator_core::video::FrameBuffer;

use crate::bus::IRQ_SERIAL;
use crate::gb::Gb;

/// Kennung der Save-States des Linkbetriebs.
pub const LINK_STATE_MAGIC: [u8; 4] = *b"KGBL";

pub struct GbLink {
    pub gbs: [Gb; 2],
    selected: usize,
    framebuffer: FrameBuffer,
}

impl GbLink {
    /// Zwei Geräte mit je einem ROM (auch zweimal dasselbe).
    pub fn new(rom_a: &[u8], rom_b: &[u8]) -> Result<Self, String> {
        let mut link = Self {
            gbs: [Gb::from_rom(rom_a)?, Gb::from_rom(rom_b)?],
            selected: 0,
            framebuffer: FrameBuffer::new(320, 144),
        };
        link.connect();
        Ok(link)
    }

    fn connect(&mut self) {
        for i in 0..2 {
            let peer = self.gbs[1 - i].bus.serial.out_bit();
            self.gbs[i].bus.serial.peer = Some(peer);
        }
    }

    /// Ein Schritt des zurückliegenden Geräts samt Weitergabe der Taktimpulse.
    fn step(&mut self) -> (usize, Option<StopReason>) {
        let i = usize::from(self.gbs[1].cycles() < self.gbs[0].cycles());
        let peer_bit = self.gbs[1 - i].bus.serial.out_bit();
        self.gbs[i].bus.serial.peer = Some(peer_bit);
        let stop = self.gbs[i].link_step();
        let pulses = std::mem::take(&mut self.gbs[i].bus.serial.pulses);
        for bit in pulses {
            let other = &mut self.gbs[1 - i].bus;
            if other.serial.external_pulse(bit) {
                other.if_ |= IRQ_SERIAL;
            }
        }
        (i, stop)
    }

    fn compose(&mut self) {
        let (w0, h0) = (
            self.gbs[0].framebuffer().width(),
            self.gbs[0].framebuffer().height(),
        );
        if self.framebuffer.width() != w0 * 2 || self.framebuffer.height() != h0 {
            self.framebuffer.resize(w0 * 2, h0);
        }
        for (n, gb) in self.gbs.iter().enumerate() {
            let fb = gb.framebuffer();
            for y in 0..fb.height().min(h0) {
                for x in 0..fb.width().min(w0) {
                    let [r, g, b, _] = fb.pixel(x, y);
                    self.framebuffer.set_pixel(n as u32 * w0 + x, y, [r, g, b]);
                }
            }
        }
    }

    fn sel(&self) -> &Gb {
        &self.gbs[self.selected]
    }

    fn sel_mut(&mut self) -> &mut Gb {
        &mut self.gbs[self.selected]
    }

    pub fn save_state_bytes(&self) -> Vec<u8> {
        let mut w = StateWriter::new(LINK_STATE_MAGIC, crate::gb::STATE_VERSION);
        self.save_state(&mut w);
        w.finish()
    }

    pub fn load_state_bytes(&mut self, data: &[u8]) -> Result<(), StateError> {
        let mut r = StateReader::new(data, LINK_STATE_MAGIC, crate::gb::STATE_VERSION)?;
        self.load_state(&mut r)
    }
}

impl Machine for GbLink {
    fn info(&self) -> MachineInfo {
        let mut info = self.gbs[0].info();
        info.name = format!("{} (Linkkabel)", info.name);
        info
    }

    fn reset(&mut self) {
        for gb in &mut self.gbs {
            gb.reset();
        }
        self.connect();
    }

    fn load_program(&mut self, bytes: &[u8]) -> Result<(), String> {
        for gb in &mut self.gbs {
            gb.load_program(bytes)?;
        }
        self.connect();
        Ok(())
    }

    fn set_input(&mut self, input: &Input) {
        for (n, gb) in self.gbs.iter_mut().enumerate() {
            let mut single = Input::default();
            single.pads[0] = input.pads[n];
            gb.set_input(&single);
        }
    }

    fn run_frame(&mut self) -> StopReason {
        let mut done = [false; 2];
        loop {
            let (i, stop) = self.step();
            if let Some(s) = stop {
                self.compose();
                return s;
            }
            if self.gbs[i].take_frame_done() {
                done[i] = true;
            }
            if done[0] && self.gbs[1].cycles() >= self.gbs[0].cycles() {
                break;
            }
        }
        self.compose();
        StopReason::FrameDone
    }

    fn run(&mut self, max_steps: u64) -> StopReason {
        for _ in 0..max_steps {
            if let (_, Some(s)) = self.step() {
                self.compose();
                return s;
            }
        }
        self.compose();
        StopReason::StepLimit
    }

    fn framebuffer(&self) -> &FrameBuffer {
        &self.framebuffer
    }

    fn drain_audio(&mut self, out: &mut AudioBuffer) {
        self.gbs[0].drain_audio(out);
        let mut discard = AudioBuffer::new(out.sample_rate());
        self.gbs[1].drain_audio(&mut discard);
    }

    fn cycles(&self) -> u64 {
        self.gbs[0].cycles()
    }

    fn tty_output(&self) -> &str {
        self.sel().tty_output()
    }
}

impl DebugTarget for GbLink {
    fn cpus(&self) -> &'static [&'static str] {
        &["Gerät 1", "Gerät 2"]
    }

    fn selected_cpu(&self) -> usize {
        self.selected
    }

    fn select_cpu(&mut self, index: usize) -> bool {
        if index < 2 {
            self.selected = index;
            true
        } else {
            false
        }
    }

    fn registers(&self) -> &'static [RegisterInfo] {
        self.sel().registers()
    }

    fn read_register(&self, id: usize) -> u64 {
        self.sel().read_register(id)
    }

    fn write_register(&mut self, id: usize, value: u64) -> bool {
        self.sel_mut().write_register(id, value)
    }

    fn graphics_views(&self) -> Vec<GraphicsView> {
        self.sel().graphics_views()
    }

    fn pc(&self) -> u32 {
        self.sel().pc()
    }

    fn memory_regions(&self) -> &'static [MemoryRegion] {
        self.sel().memory_regions()
    }

    fn peek(&self, addr: u32, buf: &mut [u8]) {
        self.sel().peek(addr, buf)
    }

    fn poke(&mut self, addr: u32, data: &[u8]) -> Result<(), String> {
        self.sel_mut().poke(addr, data)
    }

    fn disassemble(
        &self,
        addr: u32,
        count: usize,
        symbols: Option<&SymbolTable>,
    ) -> Vec<DisasmLine> {
        self.sel().disassemble(addr, count, symbols)
    }

    fn call_stack(&self) -> Vec<StackFrame> {
        self.sel().call_stack()
    }

    fn breakpoints(&self) -> &Breakpoints {
        self.sel().breakpoints()
    }

    fn breakpoints_mut(&mut self) -> &mut Breakpoints {
        self.sel_mut().breakpoints_mut()
    }

    /// Einzelschritte laufen nur auf dem gewählten Gerät; das andere wartet.
    fn step_into(&mut self) -> StopReason {
        self.sel_mut().step_into()
    }

    fn step_over(&mut self) -> StopReason {
        self.sel_mut().step_over()
    }

    fn step_out(&mut self) -> StopReason {
        self.sel_mut().step_out()
    }
}

impl SaveState for GbLink {
    fn save_state(&self, w: &mut StateWriter) {
        w.u8(self.selected as u8);
        for gb in &self.gbs {
            w.blob(&gb.save_state_bytes());
        }
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.selected = usize::from(r.u8()? & 1);
        for gb in &mut self.gbs {
            let data = r.blob()?;
            gb.load_state_bytes(&data)?;
        }
        self.connect();
        self.compose();
        Ok(())
    }
}
