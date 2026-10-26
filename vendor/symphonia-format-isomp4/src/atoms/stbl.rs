// Symphonia
// Copyright (c) 2019-2026 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::atoms::{
    Atom, AtomHeader, AtomIterator, AtomType, Co64Atom, ReadAtom, Result, StcoAtom, StscAtom,
    StsdAtom, StszAtom, SttsAtom, decode_error,
};

use super::stsd::SampleEntry;
use super::stsz::SampleSize;
use log::{debug, warn};

/// Sample table atom.
#[allow(dead_code)]
#[derive(Debug)]
pub struct StblAtom {
    pub stsd: StsdAtom,
    pub stts: SttsAtom,
    pub stsc: StscAtom,
    pub stsz: StszAtom,
    pub stco: Option<StcoAtom>,
    pub co64: Option<Co64Atom>,
}

impl Atom for StblAtom {
    fn read<R: ReadAtom>(it: &mut AtomIterator<R>, _header: &AtomHeader) -> Result<Self> {
        let mut stsd = None;
        let mut stts = None;
        let mut stsc = None;
        let mut stsz = None;
        let mut stco = None;
        let mut co64 = None;

        while let Some(header) = it.next_header()? {
            match header.atom_type {
                AtomType::SampleDescription => {
                    stsd = Some(it.read_atom::<StsdAtom>()?);
                }
                AtomType::TimeToSample => {
                    stts = Some(it.read_atom::<SttsAtom>()?);
                }
                AtomType::CompositionTimeToSample => {
                    // Composition time to sample atom is only required for video.
                    debug!("ignoring ctts atom.");
                }
                AtomType::SyncSample => {
                    // Sync sample atom is only required for video.
                    debug!("ignoring stss atom.");
                }
                AtomType::SampleToChunk => {
                    stsc = Some(it.read_atom::<StscAtom>()?);
                }
                AtomType::SampleSize => {
                    stsz = Some(it.read_atom::<StszAtom>()?);
                }
                AtomType::ChunkOffset => {
                    stco = Some(it.read_atom::<StcoAtom>()?);
                }
                AtomType::ChunkOffset64 => {
                    co64 = Some(it.read_atom::<Co64Atom>()?);
                }
                _ => (),
            }
        }

        if stsd.is_none() {
            return decode_error("isomp4 (stbl): missing stsd atom");
        }

        if stts.is_none() {
            return decode_error("isomp4 (stbl): missing stts atom");
        }

        if stsc.is_none() {
            return decode_error("isomp4 (stbl): missing stsc atom");
        }

        if stsz.is_none() {
            return decode_error("isomp4 (stbl): missing stsz atom");
        }

        if stco.is_none() && co64.is_none() {
            // This is a spec. violation, but some m4a files appear to lack these atoms.
            warn!("missing stco or co64 atom");
        }

        let stsd = stsd.unwrap();
        let mut stsz = stsz.unwrap();
        // QuickTime's original sound sample tables describe nominal one-byte
        // samples. For uncompressed PCM, one media sample is actually a frame
        // containing one sample per channel. Normalize the stride before both
        // sequential reading and seeking calculate byte offsets.
        // https://developer.apple.com/documentation/quicktime-file-format/sound_sample_description_version_1
        if let SampleEntry::Audio(entry) = &stsd.sample_entry {
            if let (Some(frame_size), SampleSize::Constant(1)) =
                (entry.legacy_pcm_frame_size, &stsz.sample_sizes)
            {
                stsz.sample_sizes = SampleSize::Constant(frame_size);
            }
        }

        Ok(StblAtom {
            stsd,
            stts: stts.unwrap(),
            stsc: stsc.unwrap(),
            stsz,
            stco,
            co64,
        })
    }
}
