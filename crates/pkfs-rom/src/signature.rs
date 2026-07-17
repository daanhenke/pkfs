//! Detection of Nitro/NDS file formats from their leading 4-byte stamp.

/// A recognised Nitro/NDS file format. Container formats keep their stamp
/// forward ("NARC"); the 2D graphics formats store theirs byte-reversed on disk
/// ("NCLR" -> "RLCN").
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Signature {
    Unknown,
    Narc,
    Nclr,
    Ncgr,
    Nscr,
    Ncer,
    Nanr,
    Nmcr,
    Nmar,
    Nsbmd,
    Nsbtx,
    Nsbca,
    Nsbtp,
    Nsbta,
    Nsbma,
    Nsbva,
    Sdat,
}

struct Entry {
    stamp: &'static [u8; 4],
    sig: Signature,
    label: &'static str,
    ext: &'static str,
}

const TABLE: &[Entry] = &[
    Entry {
        stamp: b"NARC",
        sig: Signature::Narc,
        label: "narc",
        ext: "narc",
    },
    Entry {
        stamp: b"RLCN",
        sig: Signature::Nclr,
        label: "nclr",
        ext: "nclr",
    },
    Entry {
        stamp: b"RGCN",
        sig: Signature::Ncgr,
        label: "ncgr",
        ext: "ncgr",
    },
    Entry {
        stamp: b"RCSN",
        sig: Signature::Nscr,
        label: "nscr",
        ext: "nscr",
    },
    Entry {
        stamp: b"RECN",
        sig: Signature::Ncer,
        label: "ncer",
        ext: "ncer",
    },
    Entry {
        stamp: b"RNAN",
        sig: Signature::Nanr,
        label: "nanr",
        ext: "nanr",
    },
    Entry {
        stamp: b"RCMN",
        sig: Signature::Nmcr,
        label: "nmcr",
        ext: "nmcr",
    },
    Entry {
        stamp: b"RAMN",
        sig: Signature::Nmar,
        label: "nmar",
        ext: "nmar",
    },
    Entry {
        stamp: b"BMD0",
        sig: Signature::Nsbmd,
        label: "nsbmd",
        ext: "nsbmd",
    },
    Entry {
        stamp: b"BTX0",
        sig: Signature::Nsbtx,
        label: "nsbtx",
        ext: "nsbtx",
    },
    Entry {
        stamp: b"BCA0",
        sig: Signature::Nsbca,
        label: "nsbca",
        ext: "nsbca",
    },
    Entry {
        stamp: b"BTP0",
        sig: Signature::Nsbtp,
        label: "nsbtp",
        ext: "nsbtp",
    },
    Entry {
        stamp: b"BTA0",
        sig: Signature::Nsbta,
        label: "nsbta",
        ext: "nsbta",
    },
    Entry {
        stamp: b"BMA0",
        sig: Signature::Nsbma,
        label: "nsbma",
        ext: "nsbma",
    },
    Entry {
        stamp: b"BVA0",
        sig: Signature::Nsbva,
        label: "nsbva",
        ext: "nsbva",
    },
    Entry {
        stamp: b"SDAT",
        sig: Signature::Sdat,
        label: "sdat",
        ext: "sdat",
    },
];

/// Identify the format of a blob from its leading stamp.
pub fn detect_signature(data: &[u8]) -> Signature {
    if data.len() < 4 {
        return Signature::Unknown;
    }
    for e in TABLE {
        if &data[..4] == e.stamp {
            return e.sig;
        }
    }
    Signature::Unknown
}

pub fn signature_label(sig: Signature) -> &'static str {
    TABLE
        .iter()
        .find(|e| e.sig == sig)
        .map_or("unknown", |e| e.label)
}

pub fn signature_extension(sig: Signature) -> &'static str {
    TABLE.iter().find(|e| e.sig == sig).map_or("bin", |e| e.ext)
}

pub fn is_narc(data: &[u8]) -> bool {
    detect_signature(data) == Signature::Narc
}
