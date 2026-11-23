//! Modelle der Plattform.

/// Emuliertes Gerät.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    /// Game Boy (DMG-B).
    Dmg,
    /// Game Boy Color (CGB-E).
    Cgb,
    /// Super Game Boy (Takt vom SNES abgeleitet).
    Sgb,
    /// Super Game Boy 2.
    Sgb2,
}

impl Model {
    pub fn is_cgb(self) -> bool {
        self == Model::Cgb
    }

    pub fn is_sgb(self) -> bool {
        matches!(self, Model::Sgb | Model::Sgb2)
    }

    /// Wählt das Modell nach dem Kassettenkopf: CGB-Flag vor SGB-Flag.
    pub fn for_header(cgb_flag: u8, sgb_flag: u8) -> Self {
        if cgb_flag & 0x80 != 0 {
            Model::Cgb
        } else if sgb_flag == 0x03 {
            Model::Sgb
        } else {
            Model::Dmg
        }
    }

    /// CPU-Takt in Hz (Normaltakt).
    pub fn cpu_hz(self) -> u64 {
        match self {
            // SGB1: 21,477272 MHz (SNES) / 5
            Model::Sgb => 4_295_454,
            _ => 4_194_304,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Model::Dmg => "Game Boy",
            Model::Cgb => "Game Boy Color",
            Model::Sgb => "Super Game Boy",
            Model::Sgb2 => "Super Game Boy 2",
        }
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<Self> {
        [Model::Dmg, Model::Cgb, Model::Sgb, Model::Sgb2]
            .get(usize::from(id))
            .copied()
    }
}
