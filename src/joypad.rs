//! Joypad über `P1` (`FF00`): zwei Zeilen à vier Tasten, aktiv low.

use kupocode_emulator_core::input::Buttons;
use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

/// Zuordnung der neutralen Tasten (wie beim NES: A rechts, B unten).
const ACTION: [Buttons; 4] = [
    Buttons::EAST,
    Buttons::SOUTH,
    Buttons::SELECT,
    Buttons::START,
];
const DIRECTION: [Buttons; 4] = [Buttons::RIGHT, Buttons::LEFT, Buttons::UP, Buttons::DOWN];

#[derive(Debug, Clone, Default)]
pub struct Joypad {
    /// Auswahlbits 4 und 5 von `P1`.
    pub select: u8,
    /// Gedrückte Tasten je Spieler (Mehrspieler nur beim SGB).
    pub pads: [Buttons; 4],
    /// Abgefragter Spieler.
    pub player: u8,
    /// Zuletzt gelesene untere vier Bits (für die Flanke der Unterbrechung).
    last: u8,
}

impl Joypad {
    fn lines(&self) -> u8 {
        let mut pressed = 0u8;
        let row = |keys: &[Buttons; 4]| {
            keys.iter()
                .enumerate()
                .filter(|(_, b)| self.pads[usize::from(self.player & 3)].contains(**b))
                .fold(0u8, |m, (i, _)| m | (1 << i))
        };
        if self.select & 0x10 == 0 {
            pressed |= row(&DIRECTION);
        }
        if self.select & 0x20 == 0 {
            pressed |= row(&ACTION);
        }
        !pressed & 0x0F
    }

    pub fn read(&self) -> u8 {
        0xC0 | self.select | self.lines()
    }

    pub fn write(&mut self, v: u8) {
        self.select = v & 0x30;
    }

    pub fn set_buttons(&mut self, player: usize, b: Buttons) {
        self.pads[player & 3] = b;
    }

    /// Prüft die Eingangsleitungen; `true` bei einer fallenden Flanke (Unterbrechung).
    pub fn tick(&mut self) -> bool {
        let now = self.lines();
        let irq = self.last & !now != 0;
        self.last = now;
        irq
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.u8(self.select);
        w.u8(self.last);
        w.u8(self.player);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.select = r.u8()?;
        self.last = r.u8()?;
        self.player = r.u8()?;
        Ok(())
    }
}
