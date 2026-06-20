use std::path::{Path, PathBuf};
use anyhow::Error;
use lofty::file::AudioFile;
use std::io::BufReader;
use std::fs::File;
use rodio::{Source, Decoder};
use crate::AudioSource;

pub struct MP3Source {
    path: PathBuf,
    duration: u128
}
impl MP3Source {
    pub fn new(path: impl AsRef<Path>) -> Result<MP3Source, Error> {
        // Get duration. Only read audio properties, not tags - tag parsing (e.g. lofty's
        // strict ID3 timestamp validation) can fail on files with malformed metadata, and we
        // only need the duration here.
        let file = lofty::probe::Probe::open(&path)?
            .options(lofty::config::ParseOptions::new().read_tags(false))
            .read()?;
        let duration = file.properties().duration();

        Ok(MP3Source {
            path: path.as_ref().to_owned(),
            duration: duration.as_millis()
        })
    }
}

impl AudioSource for MP3Source {
    // Get duration
    fn duration(&self) -> u128 {
        self.duration
    }

    // Get rodio decoder
    fn get_source(&self) -> Result<Box<dyn Source<Item = i16> + Send>, Error> {
        Ok(Box::new(Decoder::new_mp3(BufReader::new(File::open(&self.path)?))?))
    }
}