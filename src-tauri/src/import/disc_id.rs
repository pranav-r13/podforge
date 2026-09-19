/// One track's TOC entry as read off the disc: its length in sectors (CD-DA
/// frames, 75/sec, fixed by the Red Book spec) and the millisecond duration
/// derived from it.
#[derive(Debug, Clone)]
pub struct DiscTrack {
    pub number: i32,
    pub sectors: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone)]
pub struct DiscInfo {
    pub disc_id: String,
    pub tracks: Vec<DiscTrack>,
}

/// Reads the TOC off the disc at `device` via libdiscid and computes its
/// MusicBrainz disc ID plus per-track lengths. Named `disc_id` rather than
/// the plan's `discid` to avoid ambiguity with the `discid` crate it wraps --
/// `use`-ing an external crate from a same-named module is a known Rust
/// footgun.
pub fn read_disc(device: &str) -> Result<DiscInfo, String> {
    let disc = discid::DiscId::read(Some(device)).map_err(|e| e.to_string())?;
    let tracks = disc
        .tracks()
        .map(|t| {
            let sectors = i64::from(t.sectors);
            DiscTrack {
                number: t.number,
                sectors,
                duration_ms: (sectors * 1000) / 75,
            }
        })
        .collect();
    Ok(DiscInfo {
        disc_id: disc.id(),
        tracks,
    })
}
